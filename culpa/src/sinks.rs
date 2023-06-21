use gix::diff::blob::{intern::*, Sink};
use std::{collections::HashMap, ops::Range};

use crate::blame::LineMapping;

#[derive(Clone, Debug, Default)]
pub struct BeforeAfter {
    pub before: Range<u32>,
    pub after: Range<u32>,
}

/// Collects the ranges given to it and the old and new line contents for the
/// collected lines.
pub struct RangeAndLineCollector<'a, T>
where
    T: std::hash::Hash + std::cmp::Eq + std::fmt::Display + ToString,
{
    ranges: Vec<BeforeAfter>,
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
        for BeforeAfter { before, after } in &self.ranges {
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
    pub ranges: Vec<BeforeAfter>,
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
        self.ranges.push(BeforeAfter {
            before: before.clone(),
            after: after.clone(),
        });
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
