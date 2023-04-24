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

use crate::error::BlameDiffError;

#[derive(Debug)]
pub enum Error {
    NoFile,
}

#[derive(Debug)]
struct Line {
    line: String,
    offset: u32,
    commit: Option<gix::ObjectId>,
}

#[derive(Debug)]
struct IncompleteBlame {
    x: std::collections::BTreeMap<u32, Line>,
    lines: u32,
}

impl IncompleteBlame {
    fn new(contents: String) -> Self {
        let mut x = std::collections::BTreeMap::new();
        for (line_number, line) in contents.lines().enumerate() {
            x.insert(
                (line_number + 1) as u32,
                Line {
                    line: line.to_owned(),
                    offset: 0,
                    commit: None,
                },
            );
        }

        Self {
            x,
            lines: contents.lines().count() as u32,
        }
    }

    fn assign(&mut self, line: u32, id: gix::ObjectId) {
        if let Some(l) = self.x.get_mut(&line) {
            if l.commit.is_none() {
                l.commit = Some(id);
            }
        }
    }

    fn assign_range(&mut self, lines: Range<u32>, id: gix::ObjectId) {
        for l in lines {
            self.assign(l, id);
        }
    }

    fn is_complete(&self) -> bool {
        self.x.values().all(|l| l.commit.is_some())
    }

    fn finish(self) -> Blame {
        let v = self
            .x
            .values()
            .map(|l| {
                l.commit
                    .unwrap_or(gix::ObjectId::empty_blob(hash::Kind::Sha1))
            })
            .collect::<Vec<_>>();

        Blame(v)
    }
}

/// A Blame is a set of spans
#[derive(Debug)]
pub struct Blame(Vec<gix::ObjectId>);

struct Collector<'a> {
    interner: &'a Interner<&'a str>,
    ranges: Vec<(Range<u32>, Range<u32>)>,
}

impl<'a> Collector<'a> {
    fn new(input: &'a InternedInput<&str>) -> Self {
        Self {
            interner: &input.interner,
            ranges: vec![],
        }
    }
}

impl<'a> Sink for Collector<'a> {
    type Out = Vec<(Range<u32>, Range<u32>)>;

    fn process_change(&mut self, before: Range<u32>, after: Range<u32>) {
        self.ranges.push((before, after));
    }

    fn finish(self) -> Self::Out {
        self.ranges
    }
}

pub fn blame_file(revision: &str, path: &Path) -> Result<Blame, crate::BlameDiffError> {
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

    let mut blame_state = IncompleteBlame::new(contents.clone());

    let mut iter = repo
        .rev_walk(std::iter::once(head))
        .all()
        .unwrap()
        .peekable();

    while let Some(Ok(c_id)) = iter.next() {
        if blame_state.is_complete() {
            break;
        }

        let commit = repo
            .find_object(c_id)?
            .peel_to_kind(object::Kind::Commit)?
            .into_commit();

        if let Some(tree_entry) = commit.tree()?.lookup_entry_by_path(path).unwrap() {
            if let Some(Ok(prev_commit_id)) = iter.peek() {
                let prev_commit_id = prev_commit_id.as_ref();

                let prev_commit = repo
                    .find_object(prev_commit_id)?
                    .peel_to_kind(object::Kind::Commit)?
                    .into_commit();

                if let Some(prev_tree_entry) = prev_commit.tree()?.lookup_entry_by_path(path)? {
                    if tree_entry.object_id() != prev_tree_entry.object_id() {
                        let old = &prev_tree_entry.object()?.data;
                        let new = &tree_entry.object()?.data;

                        let old_file = std::str::from_utf8(&old).expect("valid UTF-8");
                        let new_file = std::str::from_utf8(&new).expect("valid UTF-8");

                        let input = InternedInput::new(old_file, new_file);

                        let ranges = diff(Algorithm::Histogram, &input, Collector::new(&input));

                        for (_before, after) in ranges.into_iter() {
                            blame_state.assign_range(after, c_id.detach());
                        }
                    }
                }
            } else {
                // File doesn't exist in previous commit
                // Attribute remainling lines to this commit
                blame_state.assign_range(1..(blame_state.lines as u32 + 1), c_id.detach());
            }
        } else {
            // File doesn't exist in current commit
            break;
        }
    }

    let b = blame_state.finish();

    for (a, b) in contents.lines().zip(b.0.iter()) {
        println!("{}\t{}", b, a);
    }

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
