#![allow(unused_must_use)]
#![allow(dead_code)]
#![allow(unused_imports)]

use std::{ops::Range, path::Path};

use gix::{
    bstr,
    diff::blob::{
        diff,
        intern::{InternedInput, Interner, Token},
        Algorithm, Sink,
    },
    discover, hash, index, object, objs, Id, Object, Repository,
};

use rangemap::RangeMap;

use crate::collector;
use crate::error::BlameDiffError;

/// A Blame represents a list of commit IDs, one for each line of the file.
#[derive(Debug)]
pub struct Blame(Vec<gix::ObjectId>);

struct Line {
    offset: i32,
    content: String,
}

#[derive(Debug)]
struct IncompleteBlame {
    wip: RangeMap<u32, gix::ObjectId>,
    offsets: RangeMap<u32, i32>,
    lines: Vec<String>,
    total_range: Range<u32>,
}

impl IncompleteBlame {
    fn new(contents: String) -> Self {
        let lines: Vec<String> = contents.lines().map(|s| s.to_string()).collect();

        let len = lines.len() as u32;
        let total_range = 0..len;

        let offsets = RangeMap::from_iter(std::iter::once((total_range.clone(), 0)));

        Self {
            wip: RangeMap::new(),
            offsets,
            lines,
            total_range,
        }
    }

    fn assign(&mut self, lines: Range<u32>, id: gix::ObjectId) {
        &lines;
        let gaps = self.wip.gaps(&lines).collect::<Vec<_>>();

        for r in gaps {
            self.wip.insert(r, id)
        }
    }

    fn assign_rest(&mut self, id: gix::ObjectId) {
        let gaps = self.wip.gaps(&self.total_range).collect::<Vec<_>>();

        for r in gaps {
            self.wip.insert(r, id)
        }
    }

    fn process(&mut self, ranges: Vec<(Range<u32>, Range<u32>)>, id: gix::ObjectId) {
        for (before, after) in ranges {
            let before_len = before.end - before.start;
            let after_len = after.end - after.start;

            if before_len == after_len {
                self.assign(after, id);
            } else if before_len < after_len {
                self.assign(after, id);
            } else {
                self.assign(after, id);
            }
        }
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
) -> Result<Option<gix::object::tree::Entry<'a>>, BlameDiffError> {
    repo.find_object(id)?
        .peel_to_tree()?
        .lookup_entry_by_path(path)
        .map_err(|e| e.into())
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

    let mut iter = repo.rev_walk(std::iter::once(head)).all()?.peekable();

    while let Some(Ok(commit_id)) = iter.next() {
        if blame_state.is_complete() {
            break;
        }

        if let Some(entry) = tree_entry(&repo, commit_id, path)? {
            if let Some(Ok(prev_commit_id)) = iter.peek() {
                if let Some(prev_entry) = tree_entry(&repo, prev_commit_id.as_ref(), path)? {
                    if entry.object_id() != prev_entry.object_id() {
                        let old = &prev_entry.object()?.data;
                        let new = &entry.object()?.data;

                        let old_file = std::str::from_utf8(&old)?;
                        let new_file = std::str::from_utf8(&new)?;

                        let input = InternedInput::new(old_file, new_file);

                        let ranges =
                            diff(Algorithm::Histogram, &input, collector::Collector::new());

                        blame_state.process(ranges, commit_id.detach());
                    }
                } else {
                    // File doesn't exist in previous commit
                    // Attribute remaining lines to this commit
                    blame_state.assign_rest(commit_id.detach());
                    break;
                }
            } else {
                // There is no previous commit
                // Attribute remaining lines to this commit
                blame_state.assign_rest(commit_id.detach());
                break;
            }
        } else {
            unreachable!("File doesn't exist in current commit");
        }
    }

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

    fn run_git_blame(revision: &str) -> Vec<gix::ObjectId> {
        let output = std::process::Command::new("git")
            .args(["blame", "--no-abbrev", "--root", "-s", revision, FILE])
            .output()
            .expect("able to run git blame")
            .stdout;
        output[0..output.len() - 1]
            .split(|&c| c == b'\n')
            .map(|l| gix::ObjectId::from_hex(&l[..40]).expect("valid sha1s from git blame"))
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

                let fasit: Vec<(gix::ObjectId, String)> = fasit
                    .into_iter()
                    .zip(file.lines())
                    .map(|(f, l)| (f, l.to_string()))
                    .collect();

                let blame: Vec<(gix::ObjectId, String)> = blame
                    .into_iter()
                    .zip(file.lines())
                    .map(|(f, l)| (f, l.to_string()))
                    .collect();

                assert_eq!(fasit.len(), file.lines().count());
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
