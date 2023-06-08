use std::{ops::Range, path::Path};

use gix::{
    diff::blob::{diff, intern::InternedInput, Algorithm},
    index, object, ObjectId, Repository,
};

use rangemap::RangeMap;

use crate::collector::{Collector, Ranges};
use crate::error;
use crate::Result;

///  A line from the input file with blame information.
pub struct BlamedLine<'a> {
    /// The ID of the commit to blame for this line
    pub id: ObjectId,

    /// Whether or not this commit was a boundary commit
    pub boundary: bool,

    /// The line number of the line in the current revision
    pub line_no: usize,

    /// The line number of the line in the revision that introduced it
    pub orig_line_no: usize,

    /// The line contents themselves
    pub line: &'a str,
}

/// A Blame represents a list of blamed lines in a file. Conceptually it's a
/// list of commit IDs in the order of the lines in the file the Blame was
/// requested for.
#[derive(Debug)]
pub struct Blame {
    ids: Vec<(bool, ObjectId)>,
    contents: String,
}

impl Blame {
    /// Returns a slice of [`ObjectId`]s, one for each line of the blamed file. The
    /// list most likely contains both consecutive and non-consecutive duplicates.
    pub fn object_ids(&self) -> &[(bool, ObjectId)] {
        &self.ids
    }

    /// Returns a list of [`BlamedLine`]s.
    pub fn blamed_lines(&self) -> Vec<BlamedLine> {
        self.ids
            .iter()
            .zip(self.contents.lines().enumerate())
            .map(|(id, (line_no, line))| BlamedLine {
                id: id.1,
                boundary: id.0,
                line_no,
                orig_line_no: line_no, // TODO
                line,
            })
            .collect()
    }
}

#[derive(Debug)]
struct IncompleteBlame {
    blamed_lines: RangeMap<u32, (bool, ObjectId)>,
    total_range: Range<u32>,
    line_mapping: Vec<u32>,
    contents: String,
}

impl IncompleteBlame {
    fn new(contents: String) -> Self {
        let lines = contents.lines().count();
        let total_range = 0..lines as u32;

        Self {
            blamed_lines: RangeMap::new(),
            total_range: total_range.clone(),
            line_mapping: Vec::from_iter(total_range),
            contents,
        }
    }

    fn raw_assign(&mut self, lines: Range<u32>, boundary: bool, id: ObjectId) {
        let gaps = self.blamed_lines.gaps(&lines).collect::<Vec<_>>();

        for r in gaps {
            self.blamed_lines.insert(r, (boundary, id))
        }
    }

    fn assign(&mut self, lines: Range<u32>, id: ObjectId) {
        self.raw_assign(lines, false, id)
    }

    fn assign_rest(&mut self, id: ObjectId) {
        self.raw_assign(self.total_range.clone(), true, id)
    }

    fn process(&mut self, ranges: Vec<(Range<u32>, Range<u32>)>, id: ObjectId) {
        for (_before, after) in ranges.iter().cloned() {
            let true_ranges = self.get_true_lines(after);
            for r in true_ranges {
                self.assign(r, id);
            }
        }

        self.update_mapping(ranges);
    }

    fn is_complete(&self) -> bool {
        self.blamed_lines.gaps(&self.total_range).count() == 0
    }

    fn update_mapping(&mut self, ranges: Vec<(Range<u32>, Range<u32>)>) {
        for (before, after) in ranges {
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

    fn get_true_lines(&self, fake_lines: Range<u32>) -> Vec<Range<u32>> {
        let mut true_lines = vec![];
        for fake_line in fake_lines {
            for (true_line, mapped_line) in self.line_mapping.iter().enumerate() {
                if *mapped_line == fake_line {
                    true_lines.push(true_line as u32);
                }
            }
        }

        let mut slice: &[u32] = &true_lines;
        let mut ranges = vec![];

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

    fn finish(self) -> Blame {
        let ids = self
            .blamed_lines
            .iter()
            .flat_map(|(r, c)| r.clone().map(|_l| *c))
            .collect::<Vec<_>>();

        Blame {
            ids,
            contents: self.contents,
        }
    }
}

fn tree_entry(
    repo: &Repository,
    id: impl Into<ObjectId>,
    path: impl AsRef<Path>,
) -> Result<Option<object::tree::Entry>> {
    repo.find_object(id)?
        .peel_to_tree()?
        .lookup_entry_by_path(path)
        .map_err(|e| e.into())
}

fn diff_tree_entries(old: object::tree::Entry, new: object::tree::Entry) -> Result<Vec<Ranges>> {
    let old = &old.object()?.data;
    let new = &new.object()?.data;

    let old_file = std::str::from_utf8(old)?;
    let new_file = std::str::from_utf8(new)?;

    let input = InternedInput::new(old_file, new_file);

    Ok(diff(Algorithm::Histogram, &input, Collector::new()))
}

fn disk_newer_than_index(stat: &index::entry::Stat, path: &Path) -> Result<bool> {
    let fs_stat = std::fs::symlink_metadata(path)?;

    Ok((stat.mtime.secs as u64)
        < fs_stat
            .modified()?
            .duration_since(std::time::SystemTime::UNIX_EPOCH)?
            .as_secs())
}

/// Obtain the blame record for the given path starting from the given revision,
/// optionally limiting it at the end.
pub fn blame_file(repo: &Repository, revision: &str, path: &Path) -> Result<Blame> {
    let range = repo.rev_parse(revision)?.detach();

    use gix::revision::plumbing::Spec;
    let (start, end) = match range {
        Spec::Include(oid) => (repo.find_object(oid)?, None),
        Spec::Exclude(oid) => (repo.rev_parse_single("HEAD")?.object()?, Some(oid)),
        Spec::Range { from, to } => (repo.find_object(to)?, Some(from)),
        _ => return Err(error::Error::InvalidRange),
    };

    let rev_walker = repo.rev_walk(std::iter::once(start.id()));

    let blob = start
        .peel_to_tree()?
        .lookup_entry_by_path(path)?
        .ok_or(std::io::Error::from(std::io::ErrorKind::NotFound))?
        .object()?
        .peel_to_kind(object::Kind::Blob)?;

    let contents = std::str::from_utf8(&blob.data)?.to_string();

    let mut blame_state = IncompleteBlame::new(contents);

    let mut stop = false;
    let commits = if let Some(end) = end {
        rev_walker.selected(move |o| {
            if stop {
                return false;
            } else if end.as_ref() == o {
                stop = true;
            }
            true
        })?
    } else {
        rev_walker.all()?
    }
    .collect::<std::result::Result<Vec<_>, _>>()
    .expect("Able to collect all history");

    for c in commits.windows(2) {
        let commit = c[0];
        let prev_commit = c[1];

        let entry = tree_entry(repo, commit, path)?;
        let prev_entry = tree_entry(repo, prev_commit, path)?;

        match (entry, prev_entry) {
            (Some(e), Some(p_e)) if e.object_id() != p_e.object_id() => {
                let ranges = diff_tree_entries(p_e, e)?;
                blame_state.process(ranges, commit.detach())
            }
            (Some(_e), Some(_p_e)) => {
                // The two files are identical
                continue;
            }
            (Some(_e), None) => {
                // File doesn't exist in previous commit
                // Attribute remaining lines to this commit
                blame_state.assign_rest(commit.detach());
                break;
            }
            (None, _) => unreachable!("File doesn't exist in current commit"),
        };
    }

    // Whatever's left assign it to the last (or only) commit. In case we hit the
    // "break" above there is no rest to assign so this does nothing.
    blame_state.assign_rest(commits.last().expect("at least one commit").detach());

    if blame_state.is_complete() {
        Ok(blame_state.finish())
    } else {
        Err(error::Error::Generation)
    }
}
