use gix::diff::blob::{intern::*, Sink};
use std::{collections::HashMap, ops::Range};

pub type Ranges = (Range<u32>, Range<u32>);

/// Just collects the ranges given to it.
pub struct RangeCollector {
    ranges: Vec<Ranges>,
}

impl RangeCollector {
    pub fn new() -> Self {
        Self { ranges: vec![] }
    }
}

impl Sink for RangeCollector {
    type Out = Vec<Ranges>;

    fn process_change(&mut self, before: Range<u32>, after: Range<u32>) {
        self.ranges.push((before, after));
    }

    fn finish(self) -> Self::Out {
        self.ranges
    }
}
use crate::blame::LineMapping;

/// Collects the ranges given to it and the old and new line contents for the
/// collected lines.
pub struct RangeAndLineCollector<'a, T>
where
    T: std::hash::Hash + std::cmp::Eq + std::fmt::Display + ToString,
{
    ranges: Vec<Ranges>,
    old_lines: HashMap<u32, String>,
    new_lines: HashMap<u32, String>,

    line_mapping: LineMapping,

    interner: &'a InternedInput<T>,
}

impl<'a, T> RangeAndLineCollector<'a, T>
where
    T: std::hash::Hash + std::cmp::Eq + std::fmt::Display + ToString,
{
    pub fn new(interner: &'a InternedInput<T>, line_mapping: LineMapping) -> Self {
        Self {
            ranges: vec![],
            old_lines: HashMap::new(),
            new_lines: HashMap::new(),
            line_mapping,
            interner,
        }
    }

    fn update_mapping(&mut self) {
        for (before, after) in &self.ranges {
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
}

#[derive(Debug, Default)]
pub struct Changes {
    pub ranges: Vec<Ranges>,
    pub old_lines: HashMap<u32, String>,
    pub new_lines: HashMap<u32, String>,
    pub line_mapping: LineMapping,
}

impl<'a, T> Sink for RangeAndLineCollector<'a, T>
where
    T: std::hash::Hash + std::cmp::Eq + std::fmt::Display + ToString,
{
    type Out = Changes;

    fn process_change(&mut self, before: Range<u32>, after: Range<u32>) {
        self.ranges.push((before.clone(), after.clone()));
        for l in before {
            self.old_lines.insert(
                l,
                self.interner.interner[self.interner.before[l as usize]].to_string(),
            );
        }
        for l in after {
            self.new_lines.insert(
                l,
                self.interner.interner[self.interner.after[l as usize]].to_string(),
            );
        }
    }

    fn finish(mut self) -> Self::Out {
        self.update_mapping();

        Changes {
            ranges: self.ranges,
            old_lines: self.old_lines,
            new_lines: self.new_lines,
            line_mapping: self.line_mapping,
        }
    }
}

pub struct MappedRangeCollector {
    ranges: Vec<Ranges>,
    line_mapping: LineMapping,
}

impl MappedRangeCollector {
    pub fn new(line_mapping: LineMapping) -> Self {
        Self {
            ranges: vec![],
            line_mapping,
        }
    }

    fn update_mapping(&mut self) {
        for (before, after) in &self.ranges {
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
}

impl Sink for MappedRangeCollector {
    type Out = (Vec<Ranges>, LineMapping);

    fn process_change(&mut self, before: Range<u32>, after: Range<u32>) {
        self.ranges.push((before, after));
    }

    fn finish(mut self) -> Self::Out {
        self.update_mapping();
        (self.ranges, self.line_mapping)
    }
}
