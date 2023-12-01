use std::{collections::HashMap, ops::Range, path::Path};

use gix::{
    diff::blob::{diff, intern::InternedInput, Algorithm},
    index, object, ObjectId, Repository,
};

use rangemap::RangeMap;

use crate::{
    error,
    line_tracking::LineTracker,
    sinks::{BeforeAfter, Changes, RangeAndLineCollector},
    Result,
};

///  A line from the input file with blame information.
pub struct BlamedLine<'a> {
    /// The ID of the commit to blame for this line
    pub id: ObjectId,

    /// Whether or not this commit was a boundary commit
    pub boundary: bool,

    /// The line number of the line in the current revision
    pub line_no: usize,

    /// The line number of the line in the revision that introduced it
    pub orig_line_no: u32,

    /// The line contents themselves
    pub line: &'a str,
}

/// A Blame represents a list of blamed lines in a file. Conceptually it's a
/// list of commit IDs in the order of the lines in the file the Blame was
/// requested for.
#[derive(Debug)]
pub struct Blame {
    ids: Vec<(bool, u32, ObjectId)>,
    contents: String,
}

impl Blame {
    /// Returns a slice of [`ObjectId`]s, one for each line of the blamed file. The
    /// list most likely contains both consecutive and non-consecutive duplicates.
    pub fn object_ids(&self) -> &[(bool, u32, ObjectId)] {
        &self.ids
    }

    /// Returns a list of [`BlamedLine`]s.
    pub fn blamed_lines(&self) -> Vec<BlamedLine> {
        self.ids
            .iter()
            .zip(self.contents.lines().enumerate())
            .map(
                |((boundary, orig_line_no, id), (line_no, line))| BlamedLine {
                    id: *id,
                    boundary: *boundary,
                    line_no: line_no + 1,
                    orig_line_no: *orig_line_no + 1,
                    line,
                },
            )
            .collect()
    }
}

#[derive(Debug, PartialEq, Eq, Clone)]
struct Line {
    boundary: bool,
    original_line_no: u32,
    id: ObjectId,
}

#[derive(Debug)]
struct IncompleteBlame {
    blamed_lines: RangeMap<u32, (bool, u32, ObjectId)>,
    blamed_lines2: Vec<Option<Line>>,
    total_range: Range<u32>,
    line_trackers: HashMap<ObjectId, LineTracker>,
    contents: String,
}

impl IncompleteBlame {
    fn new(contents: String, id: gix::ObjectId) -> Self {
        let lines = contents.lines().count();
        let total_range = 0..lines as u32;

        let mut line_mappings = HashMap::new();
        line_mappings.insert(id, LineTracker::from_range(total_range.clone()));

        Self {
            blamed_lines: RangeMap::new(),
            blamed_lines2: vec![None; lines],
            total_range: total_range,
            line_trackers: line_mappings,
            contents,
        }
    }

    fn raw_assign(&mut self, lines: Range<u32>, boundary: bool, id: ObjectId) {
        let gaps = self.blamed_lines.gaps(&lines).collect::<Vec<_>>();

        for r in gaps {
            self.blamed_lines.insert(r, (boundary, 0, id))
        }

        let line_tracker = self.line_trackers.get(&id).expect("have line mapping");
        for l in lines {
            if self.blamed_lines2[l as usize].is_none() {
                self.blamed_lines2[l as usize] = Some(Line {
                    boundary,
                    original_line_no: line_tracker.get_old_line(l as u32).unwrap(),
                    id,
                });
            }
        }
    }

    fn assign(&mut self, lines: Range<u32>, id: ObjectId) {
        self.raw_assign(lines.clone(), false, id);
    }

    fn assign_as_boundary(&mut self, id: ObjectId) {
        // First remove anything that has already been assigned to this id
        // because it would have been assigned with boundary = false
        let r = self
            .blamed_lines
            .iter()
            .filter(|(_, &(_, _, idd))| idd == id)
            .map(|(r, _)| r.clone())
            .collect::<Vec<_>>();

        for r in r {
            self.blamed_lines.remove(r);
        }

        self.raw_assign(self.total_range.clone(), true, id);

        let line_tracker = self.line_trackers.get(&id).expect("have line mapping");
        dbg!(&line_tracker);
        // First remove anything that has already been assigned to this id
        // because it would have been assigned with boundary = false

        for (idx, line) in self
            .blamed_lines2
            .iter_mut()
            .enumerate()
            .filter(|(_, o)| o.is_none() || o.as_ref().unwrap().id == id)
        {
            *line = Some(Line {
                boundary: true,
                original_line_no: line_tracker.get_old_line(idx as u32).unwrap(),
                id,
            });
        }
    }

    fn process(&mut self, ranges: &[BeforeAfter], id: ObjectId) {
        for BeforeAfter { before, after } in ranges.iter().cloned() {
            let line_tracker = self.line_trackers.get(&id).expect("have line mapping");
            let true_ranges = line_tracker.get_current_lines(after.clone());
            for r in true_ranges {
                self.assign(r, id);
            }
        }
    }

    fn is_complete(&self) -> bool {
        self.blamed_lines.gaps(&self.total_range).count() == 0
            && !self.blamed_lines2.iter().any(|o| o.is_none())
    }

    fn finish(self) -> Blame {
        let ids = self
            .blamed_lines2
            .iter()
            .map(|o| {
                let o = o.as_ref().unwrap();
                (o.boundary, o.original_line_no, o.id)
            })
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
    let mut v = Vec::<u8>::new();
    repo.find_object(id)?
        .peel_to_tree()?
        .lookup_entry_by_path(path, &mut v)
        .map_err(|e| e.into())
}

fn diff_tree_entries(
    old: object::tree::Entry,
    new: object::tree::Entry,
    line_tracker: LineTracker,
) -> Result<Changes> {
    let old = &old.object()?.data;
    let new = &new.object()?.data;

    let old_file = std::str::from_utf8(old)?;
    let new_file = std::str::from_utf8(new)?;

    let input = InternedInput::new(old_file, new_file);

    Ok(diff(
        Algorithm::Histogram,
        &input,
        RangeAndLineCollector::new(&input, line_tracker),
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
            .sorting(gix::traverse::commit::Sorting::BreadthFirst);

        if first_parent {
            r.first_parent_only()
        } else {
            r
        }
    };

    let mut buf = Vec::<u8>::new();
    let start_id = start.id;
    let blob = start
        .peel_to_tree()?
        .lookup_entry_by_path(path, &mut buf)?
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
    dbg!(&commits);

    for commit_info in &commits {
        let commit = commit_info.id;
        let entry = tree_entry(repo, commit, path)?;

        let line_tracker = blame_state.line_trackers.get(&commit).unwrap().clone();
        dbg!(commit_info.id, &line_tracker);

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
                        let changes = diff_tree_entries(p_e, e.to_owned(), line_tracker.clone())?;
                        blame_state.process(&changes.ranges, commit);

                        match blame_state.line_trackers.entry(prev_commit) {
                            std::collections::hash_map::Entry::Occupied(mut o) => {
                                dbg!("merging", prev_commit);
                                o.get_mut().merge_mapping(&changes.line_tracker);
                            }
                            std::collections::hash_map::Entry::Vacant(v) => {
                                v.insert(changes.line_tracker.clone());
                            }
                        };
                    }
                    (Some(_e), Some(_p_e)) => {
                        // The two files are identical
                        blame_state
                            .line_trackers
                            .insert(prev_commit, line_tracker.clone());
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
                                diff_tree_entries(p_e, e.to_owned(), line_tracker.clone())?;

                            match blame_state.line_trackers.entry(*prev_commit) {
                                std::collections::hash_map::Entry::Occupied(mut o) => {
                                    o.get_mut().merge_mapping(&changes.line_tracker);
                                }
                                std::collections::hash_map::Entry::Vacant(v) => {
                                    v.insert(changes.line_tracker.clone());
                                }
                            };

                            merge_changes.push(changes);
                        }
                        (Some(_e), Some(_p_e)) => {
                            // The two files are identical
                            blame_state
                                .line_trackers
                                .insert(*prev_commit, line_tracker.clone());

                            merge_changes.push(Changes::default());
                        }
                        (Some(_e), None) => {
                            // File doesn't exist in previous commit
                            // Attribute remaining lines to this commit
                            blame_state
                                .line_trackers
                                .insert(*prev_commit, line_tracker.clone());
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
