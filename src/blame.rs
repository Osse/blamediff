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

#[derive(Debug)]
pub enum Error {
    NoFile,
}

#[derive(Debug)]
struct IncompleteBlame {
    x: std::collections::HashMap<usize, Option<gix::ObjectId>>,
}

impl IncompleteBlame {
    fn new(file_len: usize) -> Self {
        Self {
            x: std::collections::HashMap::new(),
        }
    }

    fn assign(&mut self, line: u32, id: gix::ObjectId) {
        self.x.insert((line as usize), Some(id));
    }

    fn assign_range(&mut self, lines: Range<u32>, id: gix::ObjectId) {
        for l in lines {
            self.assign(l, id);
        }
    }

    fn is_complete(&self) -> bool {
        !self.x.iter().any(|(a, b)| b.is_none())
    }

    fn finish(self) -> Blame {
        dbg!(&self.x);
        let mut blame = Blame(vec![]);
        let mut iter = self.x.into_iter().map(|(a, b)| b.unwrap()).enumerate();
        let (mut l, mut first) = iter.next().unwrap();

        for (ll, i) in iter {
            if i != first {
                blame.0.push(Span {
                    lines: l as u32..ll as u32,
                    commit: i,
                });

                l = ll + 1;
                first = i;
            }
        }

        blame
    }
}

/// A Blame is a set of spans
#[derive(Debug)]
pub struct Blame(Vec<Span>);

#[derive(Debug)]
/// A range of lines in the input file that is attributed to the given commit
pub struct Span {
    pub lines: Range<u32>,
    pub commit: gix::ObjectId,
}

#[derive(Debug)]
struct State {
    head: gix::ObjectId,
    state: String,
    incomplete_blame: IncompleteBlame,
}

impl State {
    fn new(_p: &Path, n: usize, head: gix::Id) -> Self {
        State {
            head: head.detach(),
            state: String::new(),
            incomplete_blame: IncompleteBlame::new(n),
        }
    }

    /// Has blamed all lines
    fn is_complete(&self) -> bool {
        self.incomplete_blame.is_complete()
    }

    fn finish(mut self) -> Blame {
        self.incomplete_blame.finish()
    }
}

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

pub fn blame_file(path: &Path) -> Result<Blame, Error> {
    let repo = discover(".").unwrap();

    let head = repo.rev_parse("HEAD").unwrap().single().unwrap();

    let old_data = head
        .object()
        .unwrap()
        .peel_to_tree()
        .unwrap()
        .lookup_entry_by_path(path)
        .unwrap()
        .unwrap()
        .object()
        .unwrap()
        .peel_to_kind(gix::object::Kind::Blob)
        .unwrap();

    let n = String::from_utf8_lossy(&old_data.data).lines().count();
    dbg!(n);

    let mut state = State::new(path, n, head);

    let mut iter = repo
        .rev_walk(std::iter::once(head))
        .all()
        .unwrap()
        .peekable();

    while let Some(c_id) = iter.next() {
        if state.is_complete() {
            break;
        }
        let c_id = c_id.unwrap();

        let c = repo
            .find_object(c_id)
            .unwrap()
            .peel_to_kind(object::Kind::Commit)
            .unwrap()
            .into_commit();

        let e = c.tree().unwrap().lookup_entry_by_path(path).unwrap();

        if let Some(e) = e {
            if let Some(aa) = iter.peek() {
                let aa = aa.as_ref().unwrap();

                let cc = repo
                    .find_object(*aa)
                    .unwrap()
                    .peel_to_kind(object::Kind::Commit)
                    .unwrap()
                    .into_commit();

                let ee = cc.tree().unwrap().lookup_entry_by_path(path).unwrap();

                if let Some(ee) = ee {
                    if e.object_id() != ee.object_id() {
                        let old = &ee.object().unwrap().data;
                        let new = &e.object().unwrap().data;

                        let old_file = std::str::from_utf8(&old).expect("valid UTF-8");
                        let new_file = std::str::from_utf8(&new).expect("valid UTF-8");

                        let input = InternedInput::new(old_file, new_file);

                        let ranges = diff(Algorithm::Histogram, &input, Collector::new(&input));

                        for (_before, after) in ranges.into_iter() {
                            state.incomplete_blame.assign_range(after, c_id.detach());
                        }
                    }
                }
            } else {
                // Root commit
                let new = &e.object().unwrap().data;
                let new_file = std::str::from_utf8(&new).expect("valid UTF-8");
                let input = InternedInput::new("", new_file);

                let ranges = diff(Algorithm::Histogram, &input, Collector::new(&input));

                for (_before, after) in ranges.into_iter() {
                    state.incomplete_blame.assign_range(after, c_id.detach());
                }
            }
        }
    }

    dbg!(&state.incomplete_blame);

    Ok(state.finish())
}
