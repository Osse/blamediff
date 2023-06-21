use std::{
    collections::{BTreeMap, HashMap},
    ops::Range,
    path::Path,
};

use gix::{
    diff::blob::{diff, intern::InternedInput, Algorithm},
    index, object, ObjectId, Repository,
};

use rangemap::RangeMap;

use crate::error;
use crate::sinks::{BeforeAfter, Changes, RangeAndLineCollector};
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

#[derive(Debug, PartialEq, Eq, Clone)]
enum Origin {
    Definitely(ObjectId),
    AncestorOf(ObjectId),
}

impl Origin {
    fn id(&self) -> ObjectId {
        match self {
            Self::Definitely(id) => *id,
            Self::AncestorOf(id) => *id,
        }
    }
}

#[derive(Debug, PartialEq, Eq, Clone)]
struct Line {
    boundary: bool,
    origin: Origin,
}

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

/// A LineMapping is map from actual line number in the blamed file to the line
/// number in a previous version.
#[derive(Clone, Default, Eq, PartialEq)]
pub struct LineMapping(Vec<u32>);

impl LineMapping {
    pub fn from_vec(v: Vec<u32>) -> Self {
        Self(v)
    }

    pub fn from_range(r: Range<u32>) -> Self {
        Self(Vec::from_iter(r))
    }

    fn get_true_lines(&self, fake_lines: Range<u32>) -> Vec<Range<u32>> {
        let mut true_lines = vec![];

        for fake_line in fake_lines {
            for (true_line, mapped_line) in self.0.iter().enumerate() {
                if *mapped_line == fake_line {
                    true_lines.push(true_line as u32);
                }
            }
        }

        let ranges = make_ranges(&true_lines);

        ranges
    }
}

impl std::ops::Deref for LineMapping {
    type Target = [u32];

    fn deref(&self) -> &[u32] {
        &self.0
    }
}

impl std::ops::DerefMut for LineMapping {
    fn deref_mut(&mut self) -> &mut [u32] {
        &mut self.0
    }
}

impl std::fmt::Debug for LineMapping {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let m: BTreeMap<usize, u32> =
            BTreeMap::from_iter(self.0.iter().enumerate().filter_map(|(i, e)| {
                if i as u32 != *e {
                    Some((i, *e))
                } else {
                    None
                }
            }));
        f.debug_struct("LineMapping")
            .field("length", &self.0.len())
            .field("map", &m)
            .finish()
    }
}

#[derive(Debug)]
struct IncompleteBlame {
    blamed_lines: RangeMap<u32, (bool, ObjectId)>,
    total_range: Range<u32>,
    line_mappings: HashMap<ObjectId, LineMapping>,
    contents: String,
}

impl IncompleteBlame {
    fn new(contents: String, id: gix::ObjectId) -> Self {
        let lines = contents.lines().count();
        let total_range = 0..lines as u32;

        let mut line_mappings = HashMap::new();
        line_mappings.insert(id, LineMapping::from_range(total_range.clone()));

        Self {
            blamed_lines: RangeMap::new(),
            total_range: total_range,
            line_mappings,
            contents,
        }
    }

    fn raw_assign(&mut self, lines: Range<u32>, boundary: bool, id: ObjectId) {
        let gaps = self.blamed_lines.gaps(&lines).collect::<Vec<_>>();
        if id == ObjectId::from_hex(b"2064b3c8178fdd1e618b2ccef061d4934d5619e4").unwrap() {
            dbg!(&gaps);
        }

        for r in gaps {
            self.blamed_lines.insert(r, (boundary, id))
        }
    }

    fn assign(&mut self, lines: Range<u32>, id: ObjectId) {
        self.raw_assign(lines, false, id)
    }

    fn assign_as_boundary(&mut self, id: ObjectId) {
        // First remove anything that has already been assigned to this id
        // because it would have been assigned with boundary = false
        let r = self
            .blamed_lines
            .iter()
            .filter(|(_, &(_, idd))| idd == id)
            .map(|(r, _)| r.clone())
            .collect::<Vec<_>>();

        for r in r {
            self.blamed_lines.remove(r);
        }

        self.raw_assign(self.total_range.clone(), true, id)
    }

    fn process(&mut self, ranges: &[BeforeAfter], id: ObjectId) {
        if id == ObjectId::from_hex(b"2064b3c8178fdd1e618b2ccef061d4934d5619e4").unwrap() {
            dbg!(ranges);
        }
        for BeforeAfter { before, after } in ranges.iter().cloned() {
            let line_mapping = self.line_mappings.get(&id).expect("have line mapping");
            let true_ranges = line_mapping.get_true_lines(after);
            if id == ObjectId::from_hex(b"2064b3c8178fdd1e618b2ccef061d4934d5619e4").unwrap() {
                dbg!(&true_ranges);
            }
            for r in true_ranges {
                self.assign(r, id);
            }
        }
    }

    fn is_complete(&self) -> bool {
        self.blamed_lines.gaps(&self.total_range).count() == 0
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

fn diff_tree_entries(
    old: object::tree::Entry,
    new: object::tree::Entry,
    line_mapping: LineMapping,
) -> Result<Changes> {
    let old = &old.object()?.data;
    let new = &new.object()?.data;

    let old_file = std::str::from_utf8(old)?;
    let new_file = std::str::from_utf8(new)?;

    let input = InternedInput::new(old_file, new_file);

    Ok(diff(
        Algorithm::Histogram,
        &input,
        RangeAndLineCollector::new(&input, line_mapping),
    ))
}

fn disk_newer_than_index(stat: &index::entry::Stat, path: &Path) -> Result<bool> {
    let fs_stat = std::fs::symlink_metadata(path)?;

    let mod_secs = fs_stat
        .modified()?
        .duration_since(std::time::SystemTime::UNIX_EPOCH)?
        .as_secs();

    Ok((stat.mtime.secs as u64) < mod_secs)
}

fn assign_blame(ib: &mut IncompleteBlame, old: gix::ObjectId, new: gix::ObjectId) {}

/// Obtain the blame record for the given path starting from the given revision,
/// optionally limiting it at the end.
pub fn blame_file(
    repo: &Repository,
    revision: &str,
    first_parent: bool,
    path: &Path,
) -> Result<Blame> {
    let range = repo.rev_parse(revision)?.detach();

    use gix::revision::plumbing::Spec;
    let (start, end) = match range {
        Spec::Include(oid) => (repo.find_object(oid)?, None),
        Spec::Exclude(oid) => (repo.rev_parse_single("HEAD")?.object()?, Some(oid)),
        Spec::Range { from, to } => (repo.find_object(to)?, Some(from)),
        _ => return Err(error::Error::InvalidRange),
    };

    let rev_walker = {
        let r = repo
            .rev_walk(std::iter::once(start.id()))
            .sorting(gix::traverse::commit::Sorting::ByCommitTimeNewestFirst);

        if first_parent {
            r.first_parent_only()
        } else {
            r
        }
    };

    let start_id = start.id;
    let blob = start
        .peel_to_tree()?
        .lookup_entry_by_path(path)?
        .ok_or(std::io::Error::from(std::io::ErrorKind::NotFound))?
        .object()?
        .peel_to_kind(object::Kind::Blob)?;

    let contents = std::str::from_utf8(&blob.data)?.to_string();

    let mut blame_state = IncompleteBlame::new(contents, start_id);

    let commits = if let Some(end) = end {
        rev_walker.selected(move |o| end.as_ref() != o)?
    } else {
        rev_walker.all()?
    }
    .collect::<std::result::Result<Vec<_>, _>>()
    .expect("Able to collect all history");

    for commit_info in &commits {
        let commit = commit_info.id;
        let entry = tree_entry(repo, commit, path)?;

        let line_mapping = blame_state.line_mappings.get(&commit).unwrap().clone();

        match commit_info.parent_ids.len() {
            0 => {
                // Root commit (or end of range). Treat as boundary
                blame_state.assign_as_boundary(commit_info.id);
            }
            1 => {
                let prev_commit = commit_info.parent_ids[0];
                let prev_entry = tree_entry(repo, prev_commit, path)?;

                match (&entry, prev_entry) {
                    (Some(e), Some(p_e)) if e.object_id() != p_e.object_id() => {
                        let changes = diff_tree_entries(p_e, e.to_owned(), line_mapping.clone())?;
                        blame_state.process(&changes.ranges, commit);

                        if !blame_state.line_mappings.contains_key(&prev_commit) {
                            blame_state
                                .line_mappings
                                .insert(prev_commit, changes.line_mapping.clone());
                        }
                    }
                    (Some(_e), Some(_p_e)) => {
                        // The two files are identical
                        blame_state
                            .line_mappings
                            .insert(prev_commit, line_mapping.clone());
                        continue;
                    }
                    (Some(_e), None) => {
                        // File doesn't exist in previous commit
                        // Attribute remaining lines to this commit
                        blame_state.assign_as_boundary(commit);
                        break;
                    }
                    (None, _) => unreachable!("File doesn't exist in current commit"),
                };
            }
            n => {
                // This is a merge commit with n parents where n > 1
                // Collect all results *and content*
                let mut merge_changes = Vec::with_capacity(n);

                for prev_commit in &commit_info.parent_ids {
                    let prev_entry = tree_entry(repo, *prev_commit, path)?;

                    match (&entry, prev_entry) {
                        (Some(e), Some(p_e)) if e.object_id() != p_e.object_id() => {
                            let changes =
                                diff_tree_entries(p_e, e.to_owned(), line_mapping.clone())?;

                            blame_state
                                .line_mappings
                                .insert(*prev_commit, changes.line_mapping.clone());

                            merge_changes.push(changes);
                        }
                        (Some(_e), Some(_p_e)) => {
                            // The two files are identical
                            blame_state
                                .line_mappings
                                .insert(*prev_commit, line_mapping.clone());

                            merge_changes.push(Changes::default());
                        }
                        (Some(_e), None) => {
                            // File doesn't exist in previous commit
                            // Attribute remaining lines to this commit
                            blame_state
                                .line_mappings
                                .insert(*prev_commit, line_mapping.clone());
                        }
                        (None, _) => unreachable!("File doesn't exist in current commit"),
                    };
                }
            }
        }
    }

    // Whatever's left assign it to the last (or only) commit. In the case of an
    // explicit endpoint, assign to that. If we hit the "break" above there is
    // no rest to assign so this does nothing.
    if let Some(end) = end {
        blame_state.assign_as_boundary(end);
    } else {
        blame_state.assign_as_boundary(commits.last().expect("At least one commit").id);
    }

    if blame_state.is_complete() {
        Ok(blame_state.finish())
    } else {
        Err(error::Error::Generation)
    }
}
