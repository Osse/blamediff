use gix::diff::blob::Sink;
use std::ops::Range;

pub type Ranges = (Range<u32>, Range<u32>);

/// Just collects the ranges given to it.
pub struct Collector {
    ranges: Vec<Ranges>,
}

impl Collector {
    pub fn new() -> Self {
        Self { ranges: vec![] }
    }
}

impl Sink for Collector {
    type Out = Vec<Ranges>;

    fn process_change(&mut self, before: Range<u32>, after: Range<u32>) {
        self.ranges.push((before, after));
    }

    fn finish(self) -> Self::Out {
        self.ranges
    }
}
