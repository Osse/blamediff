use std::{ops::Range, path::Path};

use gix::{
    bstr,
    diff::blob::{diff, intern::InternedInput, Algorithm},
    index, object,
    revision::{
        spec::parse::{Options, RefsHint},
        Spec,
    },
    Repository,
};

use rangemap::RangeMap;

use crate::collector::{Collector, Ranges};
use crate::error::Error;

///  A line from the input file with blame information.
pub struct BlamedLine<'a> {
    /// The ID of the commit to blame for this line
    pub id: gix::ObjectId,

    /// Whether or not this commit was a boundary commit
    pub boundary: bool,

    /// The line number of the line in the current revision
    pub line_no: usize,

    /// The line number of the line in the revision that introduced it
    pub orig_line_no: usize,

    /// The line contents themselves
    pub line: &'a str,
}

/// A Blame represents a list of blamed lines in a file. Conceptually it's a
/// list of commit IDs in the order of the lines in the file the Blame was
/// requested for.
#[derive(Debug)]
pub struct Blame {
    ids: Vec<(bool, gix::ObjectId)>,
    contents: String,
}

impl Blame {
    /// Returns a slice of [`gix::ObjectId`]s, one for each line of the blamed file. The
    /// list most likely contains both consecutive and non-consecutive duplicates.
    pub fn object_ids(&self) -> &[(bool, gix::ObjectId)] {
        &self.ids
    }

    /// Returns a list of [`BlamedLine`]s.
    pub fn blamed_lines(&self) -> Vec<BlamedLine> {
        self.ids
            .iter()
            .zip(self.contents.lines().enumerate())
            .map(|(id, (line_no, line))| BlamedLine {
                id: id.1,
                boundary: id.0,
                line_no,
                orig_line_no: line_no, // TODO
                line,
            })
            .collect()
    }
}

#[derive(Debug)]
struct IncompleteBlame {
    blamed_lines: RangeMap<u32, (bool, gix::ObjectId)>,
    total_range: Range<u32>,
    line_mapping: Vec<u32>,
    contents: String,
}

impl IncompleteBlame {
    fn new(contents: String) -> Self {
        let lines = contents.lines().count();
        let total_range = 0..lines as u32;

        Self {
            blamed_lines: RangeMap::new(),
            total_range: total_range.clone(),
            line_mapping: Vec::from_iter(total_range),
            contents,
        }
    }

    fn raw_assign(&mut self, lines: Range<u32>, boundary: bool, id: gix::ObjectId) {
        let gaps = self.blamed_lines.gaps(&lines).collect::<Vec<_>>();

        for r in gaps {
            self.blamed_lines.insert(r, (boundary, id))
        }
    }

    fn assign(&mut self, lines: Range<u32>, id: gix::ObjectId) {
        self.raw_assign(lines, false, id)
    }

    fn assign_rest(&mut self, id: gix::ObjectId) {
        self.raw_assign(self.total_range.clone(), true, id)
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
        while !slice.is_empty() {
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
        let ids = self
            .blamed_lines
            .iter()
            .flat_map(|(r, c)| r.clone().map(|_l| *c))
            .collect::<Vec<_>>();

        Blame {
            ids,
            contents: self.contents,
        }
    }
}

fn tree_entry(
    repo: &Repository,
    id: impl Into<gix::ObjectId>,
    path: impl AsRef<Path>,
) -> Result<Option<object::tree::Entry>, Error> {
    repo.find_object(id)?
        .peel_to_tree()?
        .lookup_entry_by_path(path)
        .map_err(|e| e.into())
}

fn diff_tree_entries(
    old: object::tree::Entry,
    new: object::tree::Entry,
) -> Result<Vec<Ranges>, Error> {
    let old = &old.object()?.data;
    let new = &new.object()?.data;

    let old_file = std::str::from_utf8(old)?;
    let new_file = std::str::from_utf8(new)?;

    let input = InternedInput::new(old_file, new_file);

    Ok(diff(Algorithm::Histogram, &input, Collector::new()))
}

fn disk_newer_than_index(stat: &index::entry::Stat, path: &std::path::Path) -> Result<bool, Error> {
    let fs_stat = std::fs::symlink_metadata(path)?;

    Ok((stat.mtime.secs as u64)
        < fs_stat
            .modified()?
            .duration_since(std::time::SystemTime::UNIX_EPOCH)?
            .as_secs())
}

/// Obtain the blame record for the given path starting from the given revision,
/// optionally limiting it at the end.
pub fn blame_file(repo: &gix::Repository, revision: &str, path: &Path) -> Result<Blame, Error> {
    let range = repo.rev_parse(revision)?.detach();

    use gix::revision::plumbing::Spec;
    let (start, end) = match range {
        Spec::Include(oid) => (repo.find_object(oid)?, None),
        Spec::Exclude(oid) => (repo.rev_parse_single("HEAD")?.object()?, Some(oid)),
        Spec::Range { from, to } => (repo.find_object(to)?, Some(from)),
        _ => todo!(),
    };

    let rev_walker = repo.rev_walk(std::iter::once(start.id()));

    let blob = start
        .peel_to_tree()?
        .lookup_entry_by_path(path)?
        .ok_or(std::io::Error::from(std::io::ErrorKind::NotFound))?
        .object()?
        .peel_to_kind(gix::object::Kind::Blob)?;

    let contents = std::str::from_utf8(&blob.data)?.to_string();

    let mut blame_state = IncompleteBlame::new(contents);

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

        let entry = tree_entry(repo, commit, path)?;
        let prev_entry = tree_entry(repo, prev_commit, path)?;

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
        Err(Error::Generation)
    }
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;
    use std::path::Path;

    const FILE: &str = "lorem-ipsum.txt";

    // Return list of strings in the format "SHA1 SP <line contents>"
    fn run_git_blame(revision: &str) -> Vec<String> {
        let output = std::process::Command::new("git")
            .args(["-C", "..", "blame", "--line-porcelain", revision, FILE])
            .output()
            .expect("able to run git blame")
            .stdout;

        let output = std::str::from_utf8(&output).expect("valid UTF-8");

        output
            .split_terminator('\n')
            .collect::<Vec<_>>()
            .chunks(13)
            .map(|c| {
                format!(
                    "{} {} {}",
                    &c[0][..40],
                    c[10].starts_with("boundary"),
                    &c[12][1..]
                )
            })
            .collect()
    }

    fn run_git_blame_with_end(revision: &str, end: &str) -> Vec<String> {
        let range = end.to_string() + ".." + revision;
        run_git_blame(&range)
    }

    fn run_blame_file(sha1: &str, end: Option<&str>) -> super::Blame {
        let range = end
            .map(|e| e.to_string() + ".." + sha1)
            .unwrap_or(sha1.to_string());

        super::blame_file(&gix::discover(".").unwrap(), &range, Path::new(FILE)).unwrap()
    }

    fn compare(sha1: &str, blame: Vec<(bool, gix::ObjectId)>, fasit: Vec<String>, message: &str) {
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
            .map(|(id, line)| format!("{} {} {}", id.1.to_string(), id.0, line))
            .collect();

        assert_eq!(fasit.len(), blame.len(), "{}", message);
        assert_eq!(fasit, blame, "{}", message);
    }

    macro_rules! blame_test {
        ($sha1:ident, $message:literal) => {
            #[test]
            fn $sha1() {
                let sha1 = &stringify!($sha1)[4..];
                let blame = run_blame_file(sha1, None).ids;
                let fasit = run_git_blame(sha1);
                compare(sha1, blame, fasit, $message);
            }
        };
        ($sha1:ident, $sha1end:expr, $message:literal) => {
            #[test]
            fn $sha1() {
                let sha1 = &stringify!($sha1)[7..];
                let blame = run_blame_file(sha1, Some($sha1end)).ids;
                let fasit = run_git_blame_with_end(sha1, $sha1end);
                compare(sha1, blame, fasit, $message);
            }
        };
    }

    blame_test!(t01_753d1db, "Initial commit");

    blame_test!(t02_f28f649, "Simple change");
    blame_test!(t02_01_f28f649, "753d1db", "Initial commit - Simple change");

    blame_test!(t03_d3baed3, "Removes more than it adds");
    blame_test!(
        t03_01_d3baed3,
        "753d1db",
        "Initial commit - Removes more than it adds"
    );
    blame_test!(
        t03_02_d3baed3,
        "f28f649",
        "Simple change - Removes more than it adds"
    );

    blame_test!(t04_536a0f5, "Adds more than it removes");
    blame_test!(
        t04_01_536a0f5,
        "753d1db",
        "Initial commit - Adds more than it removes"
    );
    blame_test!(
        t04_02_536a0f5,
        "f28f649",
        "Simple change - Adds more than it removes"
    );
    blame_test!(
        t04_03_536a0f5,
        "d3baed3",
        "Removes more than it adds - Adds more than it removes"
    );

    blame_test!(t05_6a30c80, "Change on first line");
    blame_test!(
        t05_01_6a30c80,
        "753d1db",
        "Initial commit - Change on first line"
    );
    blame_test!(
        t05_02_6a30c80,
        "f28f649",
        "Simple change - Change on first line"
    );
    blame_test!(
        t05_03_6a30c80,
        "d3baed3",
        "Removes more than it adds - Change on first line"
    );
    blame_test!(
        t05_04_6a30c80,
        "536a0f5",
        "Adds more than it removes - Change on first line"
    );

    blame_test!(t06_4d8a3c7, "Multiple changes in one commit");
    blame_test!(
        t06_01_4d8a3c7,
        "753d1db",
        "Initial commit - Multiple changes in one commit"
    );
    blame_test!(
        t06_02_4d8a3c7,
        "f28f649",
        "Simple change - Multiple changes in one commit"
    );
    blame_test!(
        t06_03_4d8a3c7,
        "d3baed3",
        "Removes more than it adds - Multiple changes in one commit"
    );
    blame_test!(
        t06_04_4d8a3c7,
        "536a0f5",
        "Adds more than it removes - Multiple changes in one commit"
    );
    blame_test!(
        t06_05_4d8a3c7,
        "6a30c80",
        "Change on first line - Multiple changes in one commit"
    );

    blame_test!(t07_2064b3c, "Change on last line");
    blame_test!(
        t07_01_2064b3c,
        "753d1db",
        "Initial commit - Change on last line"
    );
    blame_test!(
        t07_02_2064b3c,
        "f28f649",
        "Simple change - Change on last line"
    );
    blame_test!(
        t07_03_2064b3c,
        "d3baed3",
        "Removes more than it adds - Change on last line"
    );
    blame_test!(
        t07_04_2064b3c,
        "536a0f5",
        "Adds more than it removes - Change on last line"
    );
    blame_test!(
        t07_05_2064b3c,
        "6a30c80",
        "Change on first line - Change on last line"
    );
    blame_test!(
        t07_06_2064b3c,
        "4d8a3c7",
        "Multiple changes in one commit - Change on last line"
    );

    blame_test!(t08_0e17ccb, "Blank line in context");
    blame_test!(
        t08_01_0e17ccb,
        "753d1db",
        "Initial commit - Blank line in context"
    );
    blame_test!(
        t08_02_0e17ccb,
        "f28f649",
        "Simple change - Blank line in context"
    );
    blame_test!(
        t08_03_0e17ccb,
        "d3baed3",
        "Removes more than it adds - Blank line in context"
    );
    blame_test!(
        t08_04_0e17ccb,
        "536a0f5",
        "Adds more than it removes - Blank line in context"
    );
    blame_test!(
        t08_05_0e17ccb,
        "6a30c80",
        "Change on first line - Blank line in context"
    );
    blame_test!(
        t08_06_0e17ccb,
        "4d8a3c7",
        "Multiple changes in one commit - Blank line in context"
    );
    blame_test!(
        t08_07_0e17ccb,
        "2064b3c",
        "Change on last line - Blank line in context"
    );

    blame_test!(t09_3be8265, "Indent and overlap with previous change.");
    blame_test!(
        t09_01_3be8265,
        "753d1db",
        "Initial commit - Indent and overlap with previous change."
    );
    blame_test!(
        t09_02_3be8265,
        "f28f649",
        "Simple change - Indent and overlap with previous change."
    );
    blame_test!(
        t09_03_3be8265,
        "d3baed3",
        "Removes more than it adds - Indent and overlap with previous change."
    );
    blame_test!(
        t09_04_3be8265,
        "536a0f5",
        "Adds more than it removes - Indent and overlap with previous change."
    );
    blame_test!(
        t09_05_3be8265,
        "6a30c80",
        "Change on first line - Indent and overlap with previous change."
    );
    blame_test!(
        t09_06_3be8265,
        "4d8a3c7",
        "Multiple changes in one commit - Indent and overlap with previous change."
    );
    blame_test!(
        t09_07_3be8265,
        "2064b3c",
        "Change on last line - Indent and overlap with previous change."
    );
    blame_test!(
        t09_08_3be8265,
        "0e17ccb",
        "Blank line in context - Indent and overlap with previous change."
    );

    blame_test!(t10_8bf8780, "Simple change but a bit bigger");
    blame_test!(
        t10_01_8bf8780,
        "753d1db",
        "Initial commit - Simple change but a bit bigger"
    );
    blame_test!(
        t10_02_8bf8780,
        "f28f649",
        "Simple change - Simple change but a bit bigger"
    );
    blame_test!(
        t10_03_8bf8780,
        "d3baed3",
        "Removes more than it adds - Simple change but a bit bigger"
    );
    blame_test!(
        t10_04_8bf8780,
        "536a0f5",
        "Adds more than it removes - Simple change but a bit bigger"
    );
    blame_test!(
        t10_05_8bf8780,
        "6a30c80",
        "Change on first line - Simple change but a bit bigger"
    );
    blame_test!(
        t10_06_8bf8780,
        "4d8a3c7",
        "Multiple changes in one commit - Simple change but a bit bigger"
    );
    blame_test!(
        t10_07_8bf8780,
        "2064b3c",
        "Change on last line - Simple change but a bit bigger"
    );
    blame_test!(
        t10_08_8bf8780,
        "0e17ccb",
        "Blank line in context - Simple change but a bit bigger"
    );
    blame_test!(
        t10_09_8bf8780,
        "3be8265",
        "Indent and overlap with previous change. - Simple change but a bit bigger"
    );

    blame_test!(t11_f7a3a57, "Remove a lot");
    blame_test!(t11_01_f7a3a57, "753d1db", "Initial commit - Remove a lot");
    blame_test!(t11_02_f7a3a57, "f28f649", "Simple change - Remove a lot");
    blame_test!(
        t11_03_f7a3a57,
        "d3baed3",
        "Removes more than it adds - Remove a lot"
    );
    blame_test!(
        t11_04_f7a3a57,
        "536a0f5",
        "Adds more than it removes - Remove a lot"
    );
    blame_test!(
        t11_05_f7a3a57,
        "6a30c80",
        "Change on first line - Remove a lot"
    );
    blame_test!(
        t11_06_f7a3a57,
        "4d8a3c7",
        "Multiple changes in one commit - Remove a lot"
    );
    blame_test!(
        t11_07_f7a3a57,
        "2064b3c",
        "Change on last line - Remove a lot"
    );
    blame_test!(
        t11_08_f7a3a57,
        "0e17ccb",
        "Blank line in context - Remove a lot"
    );
    blame_test!(
        t11_09_f7a3a57,
        "3be8265",
        "Indent and overlap with previous change. - Remove a lot"
    );
    blame_test!(
        t11_10_f7a3a57,
        "8bf8780",
        "Simple change but a bit bigger - Remove a lot"
    );

    blame_test!(t12_392db1b, "Add a lot and blank lines");
    blame_test!(
        t12_01_392db1b,
        "753d1db",
        "Initial commit - Add a lot and blank lines"
    );
    blame_test!(
        t12_02_392db1b,
        "f28f649",
        "Simple change - Add a lot and blank lines"
    );
    blame_test!(
        t12_03_392db1b,
        "d3baed3",
        "Removes more than it adds - Add a lot and blank lines"
    );
    blame_test!(
        t12_04_392db1b,
        "536a0f5",
        "Adds more than it removes - Add a lot and blank lines"
    );
    blame_test!(
        t12_05_392db1b,
        "6a30c80",
        "Change on first line - Add a lot and blank lines"
    );
    blame_test!(
        t12_06_392db1b,
        "4d8a3c7",
        "Multiple changes in one commit - Add a lot and blank lines"
    );
    blame_test!(
        t12_07_392db1b,
        "2064b3c",
        "Change on last line - Add a lot and blank lines"
    );
    blame_test!(
        t12_08_392db1b,
        "0e17ccb",
        "Blank line in context - Add a lot and blank lines"
    );
    blame_test!(
        t12_09_392db1b,
        "3be8265",
        "Indent and overlap with previous change. - Add a lot and blank lines"
    );
    blame_test!(
        t12_10_392db1b,
        "8bf8780",
        "Simple change but a bit bigger - Add a lot and blank lines"
    );
    blame_test!(
        t12_11_392db1b,
        "f7a3a57",
        "Remove a lot - Add a lot and blank lines"
    );

    blame_test!(t13_1050bf8, "Multiple changes in one commit again");
    blame_test!(
        t13_01_1050bf8,
        "753d1db",
        "Initial commit - Multiple changes in one commit again"
    );
    blame_test!(
        t13_02_1050bf8,
        "f28f649",
        "Simple change - Multiple changes in one commit again"
    );
    blame_test!(
        t13_03_1050bf8,
        "d3baed3",
        "Removes more than it adds - Multiple changes in one commit again"
    );
    blame_test!(
        t13_04_1050bf8,
        "536a0f5",
        "Adds more than it removes - Multiple changes in one commit again"
    );
    blame_test!(
        t13_05_1050bf8,
        "6a30c80",
        "Change on first line - Multiple changes in one commit again"
    );
    blame_test!(
        t13_06_1050bf8,
        "4d8a3c7",
        "Multiple changes in one commit - Multiple changes in one commit again"
    );
    blame_test!(
        t13_07_1050bf8,
        "2064b3c",
        "Change on last line - Multiple changes in one commit again"
    );
    blame_test!(
        t13_08_1050bf8,
        "0e17ccb",
        "Blank line in context - Multiple changes in one commit again"
    );
    blame_test!(
        t13_09_1050bf8,
        "3be8265",
        "Indent and overlap with previous change. - Multiple changes in one commit again"
    );
    blame_test!(
        t13_10_1050bf8,
        "8bf8780",
        "Simple change but a bit bigger - Multiple changes in one commit again"
    );
    blame_test!(
        t13_11_1050bf8,
        "f7a3a57",
        "Remove a lot - Multiple changes in one commit again"
    );
    blame_test!(
        t13_12_1050bf8,
        "392db1b",
        "Add a lot and blank lines - Multiple changes in one commit again"
    );
}
