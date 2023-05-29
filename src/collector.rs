use std::{ops::Range, path::Path};

use gix::diff::blob::{
    diff,
    intern::{InternedInput, Token},
    Sink,
};

pub struct Collector {
    ranges: Vec<(Range<u32>, Range<u32>)>,
}

impl<'a> Collector {
    pub fn new() -> Self {
        Self { ranges: vec![] }
    }
}

impl<'a> Sink for Collector {
    type Out = Vec<(Range<u32>, Range<u32>)>;

    fn process_change(&mut self, before: Range<u32>, after: Range<u32>) {
        self.ranges.push((before, after));
    }

    fn finish(self) -> Self::Out {
        dbg!(self.ranges)
    }
}
