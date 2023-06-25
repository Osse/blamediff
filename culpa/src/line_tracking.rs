use std::collections::BTreeMap;
use std::iter::Map;
use std::ops::Range;

fn make_ranges(mut slice: &[u32]) -> Vec<Range<u32>> {
    let mut ranges = Vec::with_capacity(slice.len());

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

#[derive(Clone, Debug, Eq, PartialEq)]
enum Mapping {
    Identity(u32),
    Shifted(u32),
    Gone,
}

impl Mapping {
    fn inner(&self) -> Option<u32> {
        match self {
            Self::Identity(s) | Self::Shifted(s) => Some(*s),
            Self::Gone => None,
        }
    }
}

/// A LineMapping is map from actual line number in the blamed file to the line
/// number in a previous version.
#[derive(Clone, Default, Eq, PartialEq)]
pub struct LineTracker(Vec<Mapping>);

impl LineTracker {
    // pub fn from_vec(v: Vec<u32>) -> Self {
    //     Self(v)
    // }

    pub fn from_range(r: Range<u32>) -> Self {
        Self(r.map(|i| Mapping::Identity(i)).collect())
    }

    pub fn get_true_lines(&self, fake_lines: Range<u32>) -> Vec<Range<u32>> {
        let mut true_lines = vec![];

        for fake_line in fake_lines {
            for (true_line, mapped_line) in self.0.iter().enumerate() {
                if mapped_line == &Mapping::Shifted(fake_line)
                    || mapped_line == &Mapping::Identity(fake_line)
                {
                    true_lines.push(true_line as u32);
                }
            }
        }

        true_lines.sort();
        let ranges = make_ranges(&true_lines);

        ranges
    }

    pub fn get_true_line(&self, fake_line: u32) -> Option<u32> {
        for (true_line, mapped_line) in self.0.iter().enumerate() {
            if mapped_line == &Mapping::Shifted(fake_line) {
                return Some(true_line as u32);
            }
        }

        None
    }

    pub fn get_fake_line(&self, true_line: u32) -> Option<u32> {
        match self.0.get(true_line as usize) {
            Some(m) => match m {
                Mapping::Identity(i) => Some(*i),
                Mapping::Shifted(s) => Some(*s),
                Mapping::Gone => None,
            },
            None => None,
        }
    }

    pub fn update_mapping(&mut self, before_after: Vec<(Range<u32>, Range<u32>)>) {
        // for (_before, after) in &before_after {
        //     for m in self
        //         .0
        //         .iter_mut()
        //         .filter(|m| after.contains(&m.inner().unwrap_or(999999)))
        //     {
        //         *m = Mapping::Gone;
        //     }
        // }

        // Collect all positions first. Otherwise the first pair of before-after
        // will shift the next pair down
        let positions = before_after
            .iter()
            .map(|(_before, after)| {
                self.0
                    .partition_point(|v| v.inner().map(|v| v < after.end).unwrap_or(true))
            })
            .collect::<Vec<_>>();

        for ((before, after), pos) in before_after.iter().zip(positions) {
            let alen = after.len();
            let blen = before.len();

            if alen > blen {
                let offset = alen - blen;

                for v in &mut self.0[pos..] {
                    *v = match v {
                        Mapping::Identity(i) => Mapping::Shifted(*i - offset as u32),
                        Mapping::Shifted(s) => Mapping::Shifted(*s - offset as u32),
                        Mapping::Gone => Mapping::Gone,
                    };
                }
            } else if blen > alen {
                let offset = blen - alen;

                for v in &mut self.0[pos..] {
                    *v = match v {
                        Mapping::Identity(i) => Mapping::Shifted(*i + offset as u32),
                        Mapping::Shifted(s) => Mapping::Shifted(*s + offset as u32),
                        Mapping::Gone => Mapping::Gone,
                    };
                }
            }
        }
    }
}

impl std::fmt::Debug for LineTracker {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let m = BTreeMap::from_iter(self.0.iter().enumerate());
        f.debug_struct("LineMapping")
            .field("length", &self.0.len())
            .field("map", &m)
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identity() {
        let lm = LineTracker::from_range(0..50);

        let r = lm.get_true_lines(0..10);

        assert_eq!(r.len(), 1);
        assert_eq!(r[0], 0..10);

        let r = lm.get_true_lines(35..43);

        assert_eq!(r.len(), 1);
        assert_eq!(r[0], 35..43);
    }

    #[test]
    fn basic() {
        let mut lm = LineTracker::from_range(0..50);

        lm.update_mapping(vec![(5..7, 5..10), (20..30, 23..25)]);

        let r = lm.get_true_lines(0..47);
        assert_eq!(r.len(), 3);
        assert_eq!(r[0], 0..5);
        assert_eq!(r[1], 10..23);
        assert_eq!(r[2], 25..42);

        dbg!(&r, &lm);

        let r = lm.get_true_lines(40..47);
        assert_eq!(r.len(), 1);
        assert_eq!(r[0], 43..50);
    }
}
