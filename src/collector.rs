use std::{ops::Range, path::Path};

use gix::diff::blob::{
    diff,
    intern::{InternedInput, Interner, Token},
    Sink,
};

pub struct Collector<'a> {
    interner: &'a Interner<&'a str>,
    ranges: Vec<(Range<u32>, Range<u32>)>,
}

impl<'a> Collector<'a> {
    pub fn new(input: &'a InternedInput<&str>) -> Self {
        Self {
            interner: &input.interner,
            ranges: vec![],
        }
    }
}

impl<'a> Sink for Collector<'a> {
    type Out = Vec<(Range<u32>, Range<u32>)>;

    fn process_change(&mut self, before: Range<u32>, after: Range<u32>) {
        self.ranges.push(dbg!((before, after)));
    }

    fn finish(self) -> Self::Out {
        self.ranges
    }
}
