#![allow(unused_must_use)]
#![allow(unused_variables)]
#![allow(dead_code)]
#![allow(unused_imports)]

use std::{
    collections::{BTreeMap, HashMap},
    ops::Range,
    path::Path,
};

use gix::{
    bstr,
    diff::blob::{
        diff,
        intern::{InternedInput, Interner, Token},
        Algorithm, Sink,
    },
    discover, hash, index, object, objs,
    odb::pack::multi_index::chunk::offsets,
    Id, Object, Repository,
};

use rangemap::RangeMap;

use crate::error::BlameDiffError;
use crate::{blame, collector};

/// A Blame represents a list of commit IDs, one for each line of the file.
#[derive(Debug)]
pub struct Blame(Vec<gix::ObjectId>);

struct Line {
    offset: i32,
    content: String,
}

#[derive(Debug)]
struct Mapper {
    /// Map from true line to fake line
    m: BTreeMap<u32, u32>,
}

impl Mapper {
    fn new(r: Range<u32>) -> Self {
        Self {
            m: BTreeMap::from_iter(r.map(|i| (i, i))),
        }
    }

    fn feed(&mut self, ranges: Vec<(Range<u32>, Range<u32>)>) {
        for (before, after) in ranges {
            let offset = after.len() as isize - before.len() as isize;
            if offset < 0 {
                for (k, v) in self.m.iter_mut() {
                    if *v >= after.end {
                        *v = *v + (-offset as u32);
                    }
                }
            } else if offset > 0 {
                for (k, v) in self.m.iter_mut() {
                    if *v >= after.end {
                        *v = *v - (offset as u32);
                    }
                }
            }
        }
    }

    fn get_true_lines(&self, fake_lines: Range<u32>) -> Vec<Range<u32>> {
        let mut true_lines = vec![];
        for l in fake_lines {
            for (k, v) in self.m.iter() {
                if *v == l {
                    true_lines.push(*k);
                }
            }
        }

        let mut slice: &[u32] = &true_lines;

        let mut ranges = vec![];

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
}

#[derive(Debug)]
struct IncompleteBlame {
    wip: RangeMap<u32, gix::ObjectId>,
    lines: Vec<String>,
    total_range: Range<u32>,
    line_mapper: Mapper,
}

impl IncompleteBlame {
    fn new(contents: String) -> Self {
        let lines: Vec<String> = contents.lines().map(|s| s.to_string()).collect();

        let len = lines.len() as u32;
        let total_range = 0..len;

        Self {
            wip: RangeMap::new(),
            lines,
            total_range: total_range.clone(),
            line_mapper: Mapper::new(total_range.clone()),
        }
    }

    fn assign(&mut self, lines: Range<u32>, id: gix::ObjectId) {
        let gaps = self.wip.gaps(&lines).collect::<Vec<_>>();

        for r in gaps {
            self.wip.insert(r, id)
        }
    }

    fn assign_rest(&mut self, id: gix::ObjectId) {
        self.assign(self.total_range.clone(), id)
    }

    fn process(&mut self, ranges: Vec<(Range<u32>, Range<u32>)>, id: gix::ObjectId) {
        for (_before, after) in ranges.iter().cloned() {
            let true_ranges = self.line_mapper.get_true_lines(after);
            for r in true_ranges {
                self.assign(r, id);
            }
        }

        self.line_mapper.feed(ranges);
    }

    fn is_complete(&self) -> bool {
        self.wip.gaps(&self.total_range).count() == 0
    }

    fn finish(self) -> Blame {
        let v = self
            .wip
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
) -> Result<Option<object::tree::Entry<'a>>, BlameDiffError> {
    repo.find_object(id)?
        .peel_to_tree()?
        .lookup_entry_by_path(path)
        .map_err(|e| e.into())
}

fn diff_tree_entries(
    old: object::tree::Entry,
    new: object::tree::Entry,
) -> Result<Vec<(Range<u32>, Range<u32>)>, BlameDiffError> {
    let old = &old.object()?.data;
    let new = &new.object()?.data;

    let old_file = std::str::from_utf8(&old)?;
    let new_file = std::str::from_utf8(&new)?;

    let input = InternedInput::new(old_file, new_file);

    Ok(diff(
        Algorithm::Histogram,
        &input,
        collector::Collector::new(),
    ))
}

pub fn blame_file(revision: &str, path: &Path) -> Result<Blame, BlameDiffError> {
    let repo = discover(".")?;

    let head = repo.rev_parse_single(revision)?;

    let blob = head
        .object()?
        .peel_to_tree()?
        .lookup_entry_by_path(path)?
        .ok_or(BlameDiffError::BadArgs)?
        .object()?
        .peel_to_kind(gix::object::Kind::Blob)?;

    let contents = String::from_utf8(blob.data.clone())?;

    let mut blame_state = IncompleteBlame::new(contents);

    let commits = repo
        .rev_walk(std::iter::once(head))
        .all()?
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

    let b = blame_state.finish();

    Ok(b)
}

#[cfg(test)]
mod tests {
    use pretty_assertions::{assert_eq, assert_ne};
    use std::fmt::Write;
    use std::path::Path;

    use super::blame_file;

    const FILE: &str = "lorem-ipsum.txt";

    fn get_file(revision: &str) -> String {
        let mut revision = revision.to_string();
        write!(&mut revision, ":{}", FILE);

        let output = std::process::Command::new("git")
            .args(["show", &revision])
            .output()
            .expect("able to run git show")
            .stdout;
        String::from_utf8(output).expect("valid UTF-8")
    }

    fn run_git_blame(revision: &str) -> Vec<String> {
        let output = std::process::Command::new("git")
            .args(["blame", "--porcelain", revision, FILE])
            .output()
            .expect("able to run git blame")
            .stdout;

        let output = std::str::from_utf8(&output).unwrap();

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

    macro_rules! blame_test {
        ($sha1:ident, $message:literal) => {
            #[test]
            fn $sha1() {
                let sha1 = &stringify!($sha1)[4..];
                let blame = blame_file(sha1, Path::new(FILE)).unwrap().0;
                let fasit = run_git_blame(sha1);

                let file = get_file(sha1);

                let blame: Vec<String> = blame
                    .into_iter()
                    .zip(file.lines())
                    .map(|(f, l)| f.to_string() + " " + l)
                    .collect();

                assert_eq!(fasit.len(), blame.len(), "{}", $message);
                assert_eq!(fasit, blame, "{}", $message);
            }
        };
    }
    blame_test!(t01_3f181d2, "Initial commit");
    blame_test!(t02_ef7c80e, "Simple change");
    blame_test!(t03_5d5d4a0, "Removes more than it adds");
    blame_test!(t04_65fd4e0, "Adds more than it removes");
    blame_test!(t05_f11d682, "Change on first line");
    blame_test!(t06_02933a0, "Change on last line");
    blame_test!(t07_45233a5, "Blank line in context");
    blame_test!(t08_8b31223, "Indent and overlap with previous change.");
    blame_test!(t09_4a881ff, "Simple change but a bit bigger");
    blame_test!(t10_00c5cf8, "Remove a lot");
    blame_test!(t11_fc492d8, "Add a lot and blank lines");
}
