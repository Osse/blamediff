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

#[derive(Debug)]
struct IncompleteBlame {
    wip: RangeMap<u32, gix::ObjectId>,
    offsets: RangeMap<u32, u32>,
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

fn find_commit<'a>(repo: &'a Repository, id: impl Into<gix::ObjectId>) -> gix::Commit<'a> {
    repo.find_object(id)
        .expect("Valid commit ID")
        .peel_to_kind(object::Kind::Commit)
        .expect("Valid commit ID")
        .into_commit()
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

    let contents = String::from_utf8(blob.data.clone()).expect("Valid UTF-8");

    let mut blame_state = IncompleteBlame::new(contents);

    let mut iter = repo
        .rev_walk(std::iter::once(head))
        .all()
        .unwrap()
        .peekable();

    while let Some(Ok(c_id)) = iter.next() {
        if blame_state.is_complete() {
            break;
        }

        let commit = find_commit(&repo, c_id);

        if let Some(tree_entry) = commit.tree()?.lookup_entry_by_path(path).unwrap() {
            if let Some(Ok(prev_commit_id)) = iter.peek() {
                let prev_commit_id = prev_commit_id.as_ref();

                let prev_commit = find_commit(&repo, prev_commit_id);

                if let Some(prev_tree_entry) = prev_commit.tree()?.lookup_entry_by_path(path)? {
                    if tree_entry.object_id() != prev_tree_entry.object_id() {
                        let old = &prev_tree_entry.object()?.data;
                        let new = &tree_entry.object()?.data;

                        let old_file = std::str::from_utf8(&old).expect("valid UTF-8");
                        let new_file = std::str::from_utf8(&new).expect("valid UTF-8");

                        let input = InternedInput::new(old_file, new_file);

                        let ranges = diff(
                            Algorithm::Histogram,
                            &input,
                            collector::Collector::new(&input),
                        );

                        for (before, after) in ranges.into_iter() {
                            let before_len = before.end - before.start;
                            let after_len = after.end - after.start;

                            if before_len == after_len {
                                blame_state.assign(after, c_id.detach());
                            } else if before_len < after_len {
                                dbg!("Lines added in this commit");
                                blame_state.assign(after, c_id.detach());
                            } else {
                                dbg!("Lines removed in this commit");
                            }
                        }
                    }
                }
            } else {
                // File doesn't exist in previous commit
                // Attribute remainling lines to this commit
                blame_state.assign_rest(c_id.detach());
            }
        } else {
            // File doesn't exist in current commit
            break;
        }
    }

    let b = blame_state.finish();

    Ok(b)
}

#[cfg(test)]
mod tests {
    use pretty_assertions::{assert_eq, assert_ne};
    use std::path::Path;

    use crate::cmd_blame;

    use super::blame_file;

    #[test]
    fn first_test() {
        let blame = std::process::Command::new("git")
            .args([
                "blame",
                "--no-abbrev",
                "--root",
                "-s",
                "first-test",
                "lorem-ipsum.txt",
            ])
            .output()
            .expect("able to run git blame")
            .stdout;

        let blame = String::from_utf8(blame)
            .expect("blame is UTF-8")
            .lines()
            .map(|l| {
                gix::ObjectId::from_hex(&l.as_bytes()[..40]).expect("valid sha1s from git blame")
            })
            .collect::<Vec<_>>();

        let p = Path::new("lorem-ipsum.txt");
        let b = blame_file("first-test", p).unwrap();

        assert_eq!(blame.len(), b.0.len());
        assert_eq!(blame, b.0);
    }
}
