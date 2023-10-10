use gix_commitgraph::Graph;
use gix_hash::{oid, ObjectId};
use gix_hashtable::hash_map::Entry;
use gix_odb::FindExt;
use gix_revwalk::{graph::IdMap, PriorityQueue};

use flagset::{flags, FlagSet};

use smallvec::SmallVec;

use ::trace::trace;
trace::init_depth_var!();

flags! {
    // Set of flags to describe the state of a particular commit while iterating.
    enum WalkFlags: u32 {
        /// Commit has been processed by the Explore walk
        Explored,
        /// Commit has been processed by the Indegree walk
        InDegree,
        /// Commit is deemed uninteresting for whatever reason
        Uninteresting,
        /// Commit marks the end of a walk, like foo in `git rev-list foo..bar`
        Bottom,
        /// TODO:
        Added,
        /// TODO:
        SymmetricLeft,
        /// TODO:
        AncestryPath,
        /// TODO: Unused?
        Seen,
    }
}

#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error("Calculated indegree missing")]
    MissingIndegree,
    #[error("Internal state not found")]
    MissingState,
    #[error("Commit not found in commit graph or storage")]
    CommitNotFound,
    #[error("Error initializing graph: {0}")]
    CommitGraphInit(#[from] gix_commitgraph::init::Error),
    #[error("Error doing file stuff: {0}")]
    CommitGraphFile(#[from] gix_commitgraph::file::commit::Error),
}

#[derive(Debug)]
struct WalkState(FlagSet<WalkFlags>);

/// A commit walker that walks in topographical order, like `git rev-list
/// --topo-order`. It requires a commit graph to be available, but not
/// necessarily up to date.
// pub struct Walk<'repo> {
pub struct Walk<Find, E>
where
    Find:
        for<'a> FnMut(&gix_hash::oid, &'a mut Vec<u8>) -> Result<gix_object::CommitRefIter<'a>, E>,
    E: std::error::Error + Send + Sync + 'static,
{
    commit_graph: Graph,
    find: Find,
    indegrees: IdMap<i32>,
    states: IdMap<WalkState>,
    explore_queue: PriorityQueue<u32, ObjectId>,
    indegree_queue: PriorityQueue<u32, ObjectId>,
    topo_queue: Vec<ObjectId>,
    min_gen: u32,
    buf: Vec<u8>,
}

#[trace(disable(new), prefix_enter = "", prefix_exit = "")]
impl<Find, E> Walk<Find, E>
where
    Find:
        for<'a> FnMut(&gix_hash::oid, &'a mut Vec<u8>) -> Result<gix_object::CommitRefIter<'a>, E>,
    E: std::error::Error + Send + Sync + 'static,
{
    /// Create a new Walk that walks the given repository, starting at the
    /// tips and ending at the bottoms. Like `git rev-list --topo-order
    /// ^bottom... tips...`
    pub fn new(
        commit_graph: gix_commitgraph::Graph,
        f: Find,
        tips: impl IntoIterator<Item = impl Into<ObjectId>>,
        ends: Option<impl IntoIterator<Item = impl Into<ObjectId>>>,
    ) -> Result<Self, Error> {
        let mut s = Self {
            commit_graph,
            find: f,
            indegrees: IdMap::default(),
            states: IdMap::default(),
            explore_queue: PriorityQueue::new(),
            indegree_queue: PriorityQueue::new(),
            topo_queue: vec![],
            min_gen: gix_commitgraph::GENERATION_NUMBER_INFINITY,
            buf: vec![],
        };

        let tips = tips.into_iter().map(Into::into).collect::<Vec<_>>();
        let tip_flags = WalkFlags::Explored | WalkFlags::InDegree;

        let end_flags = tip_flags | WalkFlags::Uninteresting | WalkFlags::Bottom;
        let ends = ends
            .map(|e| e.into_iter().map(Into::into).collect::<Vec<_>>())
            .unwrap_or_default();

        for (id, flags) in tips
            .iter()
            .map(|id| (id, tip_flags))
            .chain(ends.iter().map(|id| (id, end_flags)))
        {
            *s.indegrees.entry(*id).or_default() = 1;

            let commit = find(Some(&s.commit_graph), &mut s.find, id, &mut s.buf)
                .map_err(|_err| Error::CommitNotFound)?;

            let gen = match commit {
                Either::CommitRefIter(c) => gix_commitgraph::GENERATION_NUMBER_INFINITY,
                Either::CachedCommit(c) => c.generation(),
            };

            if gen < s.min_gen {
                s.min_gen = gen;
            }

            let state = WalkState(flags);

            s.states.insert(*id, state);
            s.explore_queue.insert(gen, *id);
            s.indegree_queue.insert(gen, *id);
        }

        s.compute_indegrees_to_depth(s.min_gen)?;

        for id in tips.iter().chain(ends.iter()) {
            let i = *s.indegrees.get(id).ok_or(Error::MissingIndegree)?;

            if i == 1 {
                s.topo_queue.push(*id);
            }
        }

        s.topo_queue.reverse();

        Ok(s)
    }

    fn compute_indegrees_to_depth(&mut self, gen_cutoff: u32) -> Result<(), Error> {
        while let Some((gen, _)) = self.indegree_queue.peek() {
            if *gen >= gen_cutoff {
                self.indegree_walk_step()?;
            } else {
                break;
            }
        }

        Ok(())
    }

    fn indegree_walk_step(&mut self) -> Result<(), Error> {
        if let Some((gen, id)) = self.indegree_queue.pop() {
            self.explore_to_depth(gen)?;

            let pgen =
                collect_parents(Some(&self.commit_graph), &mut self.find, &id, &mut self.buf)?;

            for (pid, gen) in pgen {
                self.indegrees
                    .entry(pid)
                    .and_modify(|e| *e += 1)
                    .or_insert(2);

                let state = self.states.get_mut(&pid).ok_or(Error::MissingState)?;

                if !state.0.contains(WalkFlags::InDegree) {
                    state.0 |= WalkFlags::InDegree;
                    self.indegree_queue.insert(gen, pid);
                }
            }
        }

        Ok(())
    }

    fn explore_to_depth(&mut self, gen_cutoff: u32) -> Result<(), Error> {
        while let Some((gen, _)) = self.explore_queue.peek() {
            if *gen >= gen_cutoff {
                self.explore_walk_step()?;
            } else {
                break;
            }
        }

        Ok(())
    }

    fn explore_walk_step(&mut self) -> Result<(), Error> {
        if let Some((_, id)) = self.explore_queue.pop() {
            self.process_parents(&id)?;

            let pgen =
                collect_parents(Some(&self.commit_graph), &mut self.find, &id, &mut self.buf)?;

            for (pid, gen) in pgen {
                let state = self.states.get_mut(&pid).ok_or(Error::MissingState)?;

                if !state.0.contains(WalkFlags::Explored) {
                    state.0 |= WalkFlags::Explored;
                    self.explore_queue.insert(gen, pid.into());
                }
            }
        }

        Ok(())
    }

    fn expand_topo_walk(&mut self, id: &oid) -> Result<(), Error> {
        let ret = self.process_parents(&id)?;

        // TODO: Figure out why it's correct to bail here but not other places where process_parents() is called
        if !ret {
            return Ok(());
        }

        let pgen = collect_parents(Some(&self.commit_graph), &mut self.find, &id, &mut self.buf)?;

        for (pid, parent_gen) in pgen {
            let parent_state = self.states.get(&pid).ok_or(Error::MissingState)?;

            if parent_state.0.contains(WalkFlags::Uninteresting) {
                continue;
            }

            if parent_gen < self.min_gen {
                self.min_gen = parent_gen;
                self.compute_indegrees_to_depth(self.min_gen)?;
            }

            let i = self.indegrees.get_mut(&pid).ok_or(Error::MissingIndegree)?;

            *i -= 1;

            if *i == 1 {
                self.topo_queue.push(pid);
            }
        }

        Ok(())
    }

    fn process_parents(&mut self, id: &oid) -> Result<bool, Error> {
        let state = self.states.get_mut(id).ok_or(Error::MissingState)?;

        if state.0.contains(WalkFlags::Added) {
            return Ok(true);
        }

        state.0 != WalkFlags::Added;

        let parents =
            collect_parents(Some(&self.commit_graph), &mut self.find, &id, &mut self.buf)?;

        if state.0.contains(WalkFlags::Uninteresting) {
            for (id, _) in parents {
                match self.states.entry(id) {
                    Entry::Occupied(mut o) => o.get_mut().0 |= WalkFlags::Uninteresting,
                    Entry::Vacant(v) => {
                        v.insert(WalkState(WalkFlags::Uninteresting.into()));
                    }
                };
            }

            return Ok(false);
        }

        let pass_flags =
            state.0.clone() | WalkFlags::SymmetricLeft | WalkFlags::AncestryPath | WalkFlags::Seen;

        for (id, _) in parents {
            match self.states.entry(id) {
                Entry::Occupied(mut o) => o.get_mut().0 |= pass_flags,
                Entry::Vacant(v) => {
                    v.insert(WalkState(pass_flags));
                }
            };
        }

        Ok(true)
    }
}

#[trace(prefix_enter = "", prefix_exit = "")]
impl<Find, E> Iterator for Walk<Find, E>
where
    Find:
        for<'a> FnMut(&gix_hash::oid, &'a mut Vec<u8>) -> Result<gix_object::CommitRefIter<'a>, E>,
    E: std::error::Error + Send + Sync + 'static,
{
    type Item = Result<ObjectId, Error>;

    fn next(&mut self) -> Option<Self::Item> {
        let id = self.topo_queue.pop()?;

        let i = match self.indegrees.get_mut(&id) {
            Some(i) => i,
            None => {
                return Some(Err(Error::MissingIndegree));
            }
        };

        *i = 0;

        match self.expand_topo_walk(&id) {
            Ok(_) => (),
            Err(e) => {
                return Some(Err(e));
            }
        };

        Some(Ok(id))
    }
}

enum Either<'buf, 'cache> {
    CommitRefIter(gix_object::CommitRefIter<'buf>),
    CachedCommit(gix_commitgraph::file::Commit<'cache>),
}

fn find<'cache, 'buf, Find, E>(
    cache: Option<&'cache gix_commitgraph::Graph>,
    mut find: Find,
    id: &oid,
    buf: &'buf mut Vec<u8>,
) -> Result<Either<'buf, 'cache>, E>
where
    Find:
        for<'a> FnMut(&gix_hash::oid, &'a mut Vec<u8>) -> Result<gix_object::CommitRefIter<'a>, E>,
    E: std::error::Error + Send + Sync + 'static,
{
    match cache.and_then(|cache| cache.commit_by_id(id).map(Either::CachedCommit)) {
        Some(c) => Ok(c),
        None => (find)(id, buf).map(Either::CommitRefIter),
    }
}

fn collect_parents<'b, Find, E>(
    cache: Option<&gix_commitgraph::Graph>,
    f: Find,
    id: &oid,
    buf: &'b mut Vec<u8>,
) -> Result<SmallVec<[(ObjectId, u32); 1]>, Error>
where
    Find: for<'a> FnMut(&oid, &'a mut Vec<u8>) -> Result<gix_object::CommitRefIter<'a>, E>,
    E: std::error::Error + Send + Sync + 'static,
{
    let mut pgen = SmallVec::new();

    match find(cache, f, &id, buf).map_err(|err| Error::CommitNotFound)? {
        Either::CommitRefIter(c) => {
            for token in c {
                match token {
                    Ok(gix_object::commit::ref_iter::Token::Parent { id }) => {
                        pgen.push((id, gix_commitgraph::GENERATION_NUMBER_INFINITY));
                    }
                    _ => continue,
                }
            }
        }
        Either::CachedCommit(c) => {
            for p in c.iter_parents() {
                let parent_commit = cache
                    .expect("cache exists if CachedCommit was returned")
                    .commit_at(p?);
                let pid = ObjectId::from(parent_commit.id());
                pgen.push((pid, parent_commit.generation()));
            }
        }
    };

    Ok(pgen)
}

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use super::*;
    use pretty_assertions::assert_eq;

    macro_rules! topo_test {
        ($test_name:ident, $range:literal) => {
            #[test]
            fn $test_name() {
                let repo = gix::discover(".").unwrap();
                let range = repo.rev_parse($range).expect("valid range").detach();

                use gix_revision::Spec;
                let (start, end) = match range {
                    Spec::Include(oid) => (oid, None),
                    Spec::Range { from, to } => (to, Some(from)),
                    _ => panic!("lol"),
                };

                let walk = Walk::new(
                    repo.commit_graph().unwrap(),
                    |id, buf| repo.objects.find_commit_iter(id, buf),
                    std::iter::once(start),
                    end.map(|e| std::iter::once(e)),
                )
                .unwrap();

                // let mine = walk.collect::<Result<Vec<_>, _>>().unwrap();
                // let fasit = run_git_rev_list(&[$range]);

                // assert_eq!(&mine, &fasit);
            }
        };
    }

    fn run_git_rev_list(args: &[&str]) -> Vec<gix::ObjectId> {
        let output = std::process::Command::new("git")
            .args(["rev-list", "--topo-order"])
            .args(args)
            .output()
            .expect("able to run git rev-list")
            .stdout;

        std::str::from_utf8(&output)
            .expect("sensible output from git rev-list")
            .split_terminator('\n')
            .map(gix::ObjectId::from_str)
            .collect::<Result<Vec<_>, _>>()
            .expect("rev-list returns valid object ids")
    }

    // 753d1db: Initial commit
    topo_test!(t01_753d1db, "753d1db");

    // // f28f649: Simple change
    topo_test!(t02_f28f649, "f28f649");
    topo_test!(t02_01_753d1db_f28f649, "753d1db..f28f649");

    // // d3baed3: Removes more than it adds
    topo_test!(t03_d3baed3, "d3baed3");
    topo_test!(t03_01_753d1db_d3baed3, "753d1db..d3baed3");
    topo_test!(t03_02_f28f649_d3baed3, "f28f649..d3baed3");

    // // 536a0f5: Adds more than it removes
    topo_test!(t04_536a0f5, "536a0f5");
    topo_test!(t04_01_753d1db_536a0f5, "753d1db..536a0f5");
    topo_test!(t04_02_f28f649_536a0f5, "f28f649..536a0f5");
    topo_test!(t04_03_d3baed3_536a0f5, "d3baed3..536a0f5");

    // // 6a30c80: Change on first line
    topo_test!(t05_6a30c80, "6a30c80");
    topo_test!(t05_01_753d1db_6a30c80, "753d1db..6a30c80");
    topo_test!(t05_02_f28f649_6a30c80, "f28f649..6a30c80");
    topo_test!(t05_03_d3baed3_6a30c80, "d3baed3..6a30c80");
    topo_test!(t05_04_536a0f5_6a30c80, "536a0f5..6a30c80");

    // // 4d8a3c7: Multiple changes in one commit
    topo_test!(t06_4d8a3c7, "4d8a3c7");
    topo_test!(t06_01_753d1db_4d8a3c7, "753d1db..4d8a3c7");
    topo_test!(t06_02_f28f649_4d8a3c7, "f28f649..4d8a3c7");
    topo_test!(t06_03_d3baed3_4d8a3c7, "d3baed3..4d8a3c7");
    topo_test!(t06_04_536a0f5_4d8a3c7, "536a0f5..4d8a3c7");
    topo_test!(t06_05_6a30c80_4d8a3c7, "6a30c80..4d8a3c7");

    // // 2064b3c: Change on last line
    topo_test!(t07_2064b3c, "2064b3c");
    topo_test!(t07_01_753d1db_2064b3c, "753d1db..2064b3c");
    topo_test!(t07_02_f28f649_2064b3c, "f28f649..2064b3c");
    topo_test!(t07_03_d3baed3_2064b3c, "d3baed3..2064b3c");
    topo_test!(t07_04_536a0f5_2064b3c, "536a0f5..2064b3c");
    topo_test!(t07_05_6a30c80_2064b3c, "6a30c80..2064b3c");
    topo_test!(t07_06_4d8a3c7_2064b3c, "4d8a3c7..2064b3c");

    // 0e17ccb: Blank line in context
    topo_test!(t08_0e17ccb, "0e17ccb");
    topo_test!(t08_01_753d1db_0e17ccb, "753d1db..0e17ccb");
    topo_test!(t08_02_f28f649_0e17ccb, "f28f649..0e17ccb");
    topo_test!(t08_03_d3baed3_0e17ccb, "d3baed3..0e17ccb");
    topo_test!(t08_04_536a0f5_0e17ccb, "536a0f5..0e17ccb");
    topo_test!(t08_05_6a30c80_0e17ccb, "6a30c80..0e17ccb");
    topo_test!(t08_06_4d8a3c7_0e17ccb, "4d8a3c7..0e17ccb");
    topo_test!(t08_07_2064b3c_0e17ccb, "2064b3c..0e17ccb");

    // 3be8265: Indent and overlap with previous change.
    topo_test!(t09_3be8265, "3be8265");
    topo_test!(t09_01_753d1db_3be8265, "753d1db..3be8265");
    topo_test!(t09_02_f28f649_3be8265, "f28f649..3be8265");
    topo_test!(t09_03_d3baed3_3be8265, "d3baed3..3be8265");
    topo_test!(t09_04_536a0f5_3be8265, "536a0f5..3be8265");
    topo_test!(t09_05_6a30c80_3be8265, "6a30c80..3be8265");
    topo_test!(t09_06_4d8a3c7_3be8265, "4d8a3c7..3be8265");
    topo_test!(t09_07_2064b3c_3be8265, "2064b3c..3be8265");
    topo_test!(t09_08_0e17ccb_3be8265, "0e17ccb..3be8265");

    // 8bf8780: Simple change but a bit bigger
    topo_test!(t10_8bf8780, "8bf8780");
    topo_test!(t10_01_753d1db_8bf8780, "753d1db..8bf8780");
    topo_test!(t10_02_f28f649_8bf8780, "f28f649..8bf8780");
    topo_test!(t10_03_d3baed3_8bf8780, "d3baed3..8bf8780");
    topo_test!(t10_04_536a0f5_8bf8780, "536a0f5..8bf8780");
    topo_test!(t10_05_6a30c80_8bf8780, "6a30c80..8bf8780");
    topo_test!(t10_06_4d8a3c7_8bf8780, "4d8a3c7..8bf8780");
    topo_test!(t10_07_2064b3c_8bf8780, "2064b3c..8bf8780");
    topo_test!(t10_08_0e17ccb_8bf8780, "0e17ccb..8bf8780");
    topo_test!(t10_09_3be8265_8bf8780, "3be8265..8bf8780");

    // f7a3a57: Remove a lot
    topo_test!(t11_f7a3a57, "f7a3a57");
    topo_test!(t11_01_753d1db_f7a3a57, "753d1db..f7a3a57");
    topo_test!(t11_02_f28f649_f7a3a57, "f28f649..f7a3a57");
    topo_test!(t11_03_d3baed3_f7a3a57, "d3baed3..f7a3a57");
    topo_test!(t11_04_536a0f5_f7a3a57, "536a0f5..f7a3a57");
    topo_test!(t11_05_6a30c80_f7a3a57, "6a30c80..f7a3a57");
    topo_test!(t11_06_4d8a3c7_f7a3a57, "4d8a3c7..f7a3a57");
    topo_test!(t11_07_2064b3c_f7a3a57, "2064b3c..f7a3a57");
    topo_test!(t11_08_0e17ccb_f7a3a57, "0e17ccb..f7a3a57");
    topo_test!(t11_09_3be8265_f7a3a57, "3be8265..f7a3a57");
    topo_test!(t11_10_8bf8780_f7a3a57, "8bf8780..f7a3a57");

    // 392db1b: Add a lot and blank lines
    topo_test!(t12_392db1b, "392db1b");
    topo_test!(t12_01_753d1db_392db1b, "753d1db..392db1b");
    topo_test!(t12_02_f28f649_392db1b, "f28f649..392db1b");
    topo_test!(t12_03_d3baed3_392db1b, "d3baed3..392db1b");
    topo_test!(t12_04_536a0f5_392db1b, "536a0f5..392db1b");
    topo_test!(t12_05_6a30c80_392db1b, "6a30c80..392db1b");
    topo_test!(t12_06_4d8a3c7_392db1b, "4d8a3c7..392db1b");
    topo_test!(t12_07_2064b3c_392db1b, "2064b3c..392db1b");
    topo_test!(t12_08_0e17ccb_392db1b, "0e17ccb..392db1b");
    topo_test!(t12_09_3be8265_392db1b, "3be8265..392db1b");
    topo_test!(t12_10_8bf8780_392db1b, "8bf8780..392db1b");
    topo_test!(t12_11_f7a3a57_392db1b, "f7a3a57..392db1b");

    // bb48275: Side project
    topo_test!(t13_bb48275, "bb48275");
    topo_test!(t13_01_753d1db_bb48275, "753d1db..bb48275");
    topo_test!(t13_02_f28f649_bb48275, "f28f649..bb48275");
    topo_test!(t13_03_d3baed3_bb48275, "d3baed3..bb48275");
    topo_test!(t13_04_536a0f5_bb48275, "536a0f5..bb48275");
    topo_test!(t13_05_6a30c80_bb48275, "6a30c80..bb48275");
    topo_test!(t13_06_4d8a3c7_bb48275, "4d8a3c7..bb48275");
    topo_test!(t13_07_2064b3c_bb48275, "2064b3c..bb48275");
    topo_test!(t13_08_0e17ccb_bb48275, "0e17ccb..bb48275");
    topo_test!(t13_09_3be8265_bb48275, "3be8265..bb48275");
    topo_test!(t13_10_8bf8780_bb48275, "8bf8780..bb48275");
    topo_test!(t13_11_f7a3a57_bb48275, "f7a3a57..bb48275");
    topo_test!(t13_12_392db1b_bb48275, "392db1b..bb48275");

    // c57fe89: Merge branch 'kek' into HEAD
    topo_test!(t14_c57fe89, "c57fe89");
    topo_test!(t14_01_753d1db_c57fe89, "753d1db..c57fe89");
    topo_test!(t14_02_f28f649_c57fe89, "f28f649..c57fe89");
    topo_test!(t14_03_d3baed3_c57fe89, "d3baed3..c57fe89");
    topo_test!(t14_04_536a0f5_c57fe89, "536a0f5..c57fe89");
    topo_test!(t14_05_6a30c80_c57fe89, "6a30c80..c57fe89");
    topo_test!(t14_06_4d8a3c7_c57fe89, "4d8a3c7..c57fe89");
    topo_test!(t14_07_2064b3c_c57fe89, "2064b3c..c57fe89");
    topo_test!(t14_08_0e17ccb_c57fe89, "0e17ccb..c57fe89");
    topo_test!(t14_09_3be8265_c57fe89, "3be8265..c57fe89");
    topo_test!(t14_10_8bf8780_c57fe89, "8bf8780..c57fe89");
    topo_test!(t14_11_f7a3a57_c57fe89, "f7a3a57..c57fe89");
    topo_test!(t14_12_392db1b_c57fe89, "392db1b..c57fe89");
    topo_test!(t14_13_bb48275_c57fe89, "bb48275..c57fe89");

    // d7d6328: Multiple changes in one commit again
    topo_test!(t15_d7d6328, "d7d6328");
    topo_test!(t15_01_753d1db_d7d6328, "753d1db..d7d6328");
    topo_test!(t15_02_f28f649_d7d6328, "f28f649..d7d6328");
    topo_test!(t15_03_d3baed3_d7d6328, "d3baed3..d7d6328");
    topo_test!(t15_04_536a0f5_d7d6328, "536a0f5..d7d6328");
    topo_test!(t15_05_6a30c80_d7d6328, "6a30c80..d7d6328");
    topo_test!(t15_06_4d8a3c7_d7d6328, "4d8a3c7..d7d6328");
    topo_test!(t15_07_2064b3c_d7d6328, "2064b3c..d7d6328");
    topo_test!(t15_08_0e17ccb_d7d6328, "0e17ccb..d7d6328");
    topo_test!(t15_09_3be8265_d7d6328, "3be8265..d7d6328");
    topo_test!(t15_10_8bf8780_d7d6328, "8bf8780..d7d6328");
    topo_test!(t15_11_f7a3a57_d7d6328, "f7a3a57..d7d6328");
    topo_test!(t15_12_392db1b_d7d6328, "392db1b..d7d6328");
    topo_test!(t15_13_bb48275_d7d6328, "bb48275..d7d6328");
    topo_test!(t15_14_c57fe89_d7d6328, "c57fe89..d7d6328");
}
