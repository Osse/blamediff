use std::{ops::Range, path::Path};

use gix::{
    bstr,
    diff::blob::{
        diff,
        intern::{InternedInput, Interner, Token},
        Algorithm, Sink,
    },
    discover, hash, index, object, objs,
    odb::pack::multi_index::chunk::offsets,
    revision::{
        spec::parse::{Options, RefsHint},
        Spec,
    },
    Id, Object, Repository,
};

use rangemap::RangeMap;

use crate::collector::Collector;
use crate::error::Error;

/// A Blame represents a list of commit IDs, one for each line of the file.
#[derive(Debug)]
pub struct Blame(Vec<gix::ObjectId>);

#[derive(Debug)]
struct IncompleteBlame {
    blamed_lines: RangeMap<u32, gix::ObjectId>,
    total_range: Range<u32>,
    line_mapping: Vec<u32>,
}

impl IncompleteBlame {
    fn new(lines: usize) -> Self {
        let total_range = 0..lines as u32;

        Self {
            blamed_lines: RangeMap::new(),
            total_range: total_range.clone(),
            line_mapping: Vec::from_iter(total_range),
        }
    }

    fn assign(&mut self, lines: Range<u32>, id: gix::ObjectId) {
        let gaps = self.blamed_lines.gaps(&lines).collect::<Vec<_>>();

        for r in gaps {
            self.blamed_lines.insert(r, id)
        }
    }

    fn assign_rest(&mut self, id: gix::ObjectId) {
        self.assign(self.total_range.clone(), id)
    }

    fn process(&mut self, ranges: Vec<(Range<u32>, Range<u32>)>, id: gix::ObjectId) {
        for (_before, after) in ranges.iter().cloned() {
            let true_ranges = self.get_true_lines(after);
            for r in true_ranges {
                self.assign(r, id);
            }
        }

        self.update_mapping(ranges);
    }

    fn is_complete(&self) -> bool {
        self.blamed_lines.gaps(&self.total_range).count() == 0
    }

    fn update_mapping(&mut self, ranges: Vec<(Range<u32>, Range<u32>)>) {
        for (before, after) in ranges {
            let alen = after.len();
            let blen = before.len();
            let pos = self.line_mapping.partition_point(|v| *v < after.end);

            if alen > blen {
                let offset = alen - blen;

                for v in &mut self.line_mapping[pos..] {
                    *v -= offset as u32;
                }
            } else if blen > alen {
                let offset = blen - alen;

                for v in &mut self.line_mapping[pos..] {
                    *v += offset as u32;
                }
            }
        }
    }

    fn get_true_lines(&self, fake_lines: Range<u32>) -> Vec<Range<u32>> {
        let mut true_lines = vec![];
        for fake_line in fake_lines {
            for (true_line, mapped_line) in self.line_mapping.iter().enumerate() {
                if *mapped_line == fake_line {
                    true_lines.push(true_line as u32);
                }
            }
        }

        let mut slice: &[u32] = &true_lines;
        let mut ranges = vec![];

        // TODO: When group_by becomes stable
        while slice.len() > 0 {
            let mut head_len = 1;
            let mut iter = slice.windows(2);

            while let Some([l, r]) = iter.next() {
                if *l + 1 == *r {
                    head_len += 1;
                } else {
                    break;
                }
            }

            let (head, tail) = slice.split_at(head_len);
            slice = tail;

            ranges.push(*head.first().unwrap()..*head.last().unwrap() + 1);
        }

        ranges
    }

    fn finish(self) -> Blame {
        let v = self
            .blamed_lines
            .iter()
            .flat_map(|(r, c)| r.clone().into_iter().map(|_l| c.clone()))
            .collect::<Vec<_>>();

        Blame(v)
    }
}

fn tree_entry<'a>(
    repo: &'a Repository,
    id: impl Into<gix::ObjectId>,
    path: impl AsRef<Path>,
) -> Result<Option<object::tree::Entry<'a>>, Error> {
    repo.find_object(id)?
        .peel_to_tree()?
        .lookup_entry_by_path(path)
        .map_err(|e| e.into())
}

fn diff_tree_entries(
    old: object::tree::Entry,
    new: object::tree::Entry,
) -> Result<Vec<(Range<u32>, Range<u32>)>, Error> {
    let old = &old.object()?.data;
    let new = &new.object()?.data;

    let old_file = std::str::from_utf8(&old)?;
    let new_file = std::str::from_utf8(&new)?;

    let input = InternedInput::new(old_file, new_file);

    Ok(diff(Algorithm::Histogram, &input, Collector::new()))
}

/// Obtain the blame record for the given path starting from the given revision,
/// optionally limiting it at the end.
pub fn blame_file(
    repo: &gix::Repository,
    revision: &str,
    path: &Path,
    end: Option<&str>,
) -> Result<Blame, Error> {
    let head = repo.rev_parse_single(revision)?;

    let end = end.map(|v| {
        Spec::from_bstr(
            bstr::BStr::new(v),
            &repo,
            Options {
                refs_hint: RefsHint::PreferObject,
                object_kind_hint: None,
            },
        )
        .expect("hekek")
        .single()
        .unwrap()
    });

    let blob = head
        .object()?
        .peel_to_tree()?
        .lookup_entry_by_path(path)?
        .ok_or(Error::BadArgs)?
        .object()?
        .peel_to_kind(gix::object::Kind::Blob)?;

    let contents = String::from_utf8(blob.data.clone())?;

    let mut blame_state = IncompleteBlame::new(contents.lines().count());

    let rev_walker = repo.rev_walk(std::iter::once(head));

    let mut stop = false;
    let commits = if let Some(end) = end {
        rev_walker.selected(move |o| {
            if stop {
                return false;
            } else if end.as_ref() == o {
                stop = true;
            }
            true
        })?
    } else {
        rev_walker.all()?
    }
    .collect::<Result<Vec<_>, _>>()
    .expect("Able to collect all history");

    for c in commits.windows(2) {
        let commit = c[0];
        let prev_commit = c[1];

        let entry = tree_entry(&repo, commit, path)?;
        let prev_entry = tree_entry(&repo, prev_commit, path)?;

        match (entry, prev_entry) {
            (Some(e), Some(p_e)) if e.object_id() != p_e.object_id() => {
                let ranges = diff_tree_entries(p_e, e)?;
                blame_state.process(ranges, commit.detach())
            }
            (Some(_e), Some(_p_e)) => {
                // The two files are identical
                continue;
            }
            (Some(_e), None) => {
                // File doesn't exist in previous commit
                // Attribute remaining lines to this commit
                blame_state.assign_rest(commit.detach());
                break;
            }
            (None, _) => unreachable!("File doesn't exist in current commit"),
        };
    }

    // Whatever's left assign it to the last (or only) commit. In case we hit the
    // "break" above there is no rest to assign so this does nothing.
    blame_state.assign_rest(commits.last().expect("at least one commit").detach());

    if blame_state.is_complete() {
        Ok(blame_state.finish())
    } else {
        Err(Error::BadArgs)
    }
}

#[cfg(test)]
mod tests {
    use pretty_assertions::{assert_eq, assert_ne};
    use std::path::Path;

    const FILE: &str = "lorem-ipsum.txt";

    // Return list of strings in the format "SHA1 SP <line contents>"
    fn run_git_blame(revision: &str) -> Vec<String> {
        let output = std::process::Command::new("git")
            .args(["blame", "--porcelain", revision, FILE])
            .output()
            .expect("able to run git blame")
            .stdout;

        let output = std::str::from_utf8(&output).expect("valid UTF-8");

        output
            .split_terminator('\n')
            .filter_map(|line| {
                if line.len() > 41 && line[0..40].bytes().all(|b| b.is_ascii_hexdigit()) {
                    Some(&line[0..40])
                } else if line.starts_with('\t') {
                    Some(&line[1..])
                } else {
                    None
                }
            })
            .collect::<Vec<_>>()
            .chunks(2)
            .map(|c| c[0].to_owned() + " " + c[1])
            .collect()
    }

    fn run_git_blame_with_end(revision: &str, end: &str) -> Vec<String> {
        let range = end.to_string() + ".." + revision;
        run_git_blame(&range)
    }

    fn run_blame_file(sha1: &str, end: Option<&str>) -> super::Blame {
        super::blame_file(&gix::discover(".").unwrap(), sha1, Path::new(FILE), end).unwrap()
    }

    fn compare(sha1: &str, blame: Vec<gix::ObjectId>, fasit: Vec<String>, message: &str) {
        let blob = sha1.to_string() + ":" + FILE;

        let output = std::process::Command::new("git")
            .args(["show", &blob])
            .output()
            .expect("able to run git show")
            .stdout;

        let contents = std::str::from_utf8(&output).expect("valid UTF-8");

        // Create a Vec of Strings similar to the one obtained from git itself.
        // This with pretty assertions makes it much more pleasant to debug
        let blame: Vec<String> = blame
            .into_iter()
            .zip(contents.lines())
            .map(|(id, line)| id.to_string() + " " + line)
            .collect();

        assert_eq!(fasit.len(), blame.len(), "{}", message);
        assert_eq!(fasit, blame, "{}", message);
    }

    macro_rules! blame_test {
        ($sha1:ident, $message:literal) => {
            #[test]
            fn $sha1() {
                let sha1 = &stringify!($sha1)[4..];
                let blame = run_blame_file(sha1, None).0;
                let fasit = run_git_blame(sha1);
                compare(sha1, blame, fasit, $message);
            }
        };
        ($sha1:ident, $sha1end:expr, $message:literal) => {
            #[test]
            fn $sha1() {
                let sha1 = &stringify!($sha1)[4..];
                let blame = run_blame_file(sha1, Path::new(FILE), Some($sha1end)).0;
                let fasit = run_git_blame_with_end(sha1, $sha1end);
                compare(sha1, blame, fasit, $message);
            }
        };
    }

    // git log --reverse --format='%h%x09%s' first-test | awk -F'\t' '{ printf("blame_test!(t%02d_%s, \"%s\");\n", NR, $1, $2) }'

    blame_test!(t01_753d1db, "Initial commit");
    blame_test!(t02_f28f649, "Simple change");
    blame_test!(t03_d3baed3, "Removes more than it adds");
    blame_test!(t04_536a0f5, "Adds more than it removes");
    blame_test!(t05_6a30c80, "Change on first line");
    blame_test!(t06_4d8a3c7, "Multiple changes in one commit");
    blame_test!(t07_2064b3c, "Change on last line");
    blame_test!(t08_0e17ccb, "Blank line in context");
    blame_test!(t09_3be8265, "Indent and overlap with previous change.");
    blame_test!(t10_8bf8780, "Simple change but a bit bigger");
    blame_test!(t11_f7a3a57, "Remove a lot");
    blame_test!(t12_392db1b, "Add a lot and blank lines");
    blame_test!(t13_1050bf8, "Multiple changes in one commit again");
}