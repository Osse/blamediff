use std::cmp::Reverse;

use gix_commitgraph::Graph;
use gix_hash::{oid, ObjectId};
use gix_hashtable::hash_map::Entry;
use gix_object::date::SecondsSinceUnixEpoch;
use gix_odb::FindExt;
use gix_revwalk::{graph::IdMap, PriorityQueue};

use flagset::{flags, FlagSet};

use smallvec::SmallVec;

use ::trace::trace;
trace::init_depth_var!();

flags! {
    // Set of flags to describe the state of a particular commit while iterating.
    enum WalkFlags: u32 {
        /// TODO: Unused?
        Seen,
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
    #[error("Error decoding stuff: {0}")]
    ObjectDecode(#[from] gix_object::decode::Error),
    #[error("Error ancestring stuff: {0}")]
    Ancestor(#[from] gix_traverse::commit::ancestors::Error),
}

/// Sorting to use for the topological walk
pub enum Sorting {
    /// Show no parents before all of its children are shown, but otherwise show
    /// commits in the commit timestamp order.
    DateOrder,

    /// Show no parents before all of its children are shown, and avoid
    /// showing commits on multiple lines of history intermixed.
    TopoOrder,
}

static CELL: std::sync::OnceLock<Sorting> = std::sync::OnceLock::new();

#[derive(Debug, Eq, PartialEq)]
struct Key {
    commit_time: i64,
}

impl Key {
    fn new(commit_time: i64) -> Self {
        Key { commit_time }
    }
}

impl std::cmp::Ord for Key {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.commit_time.cmp(&other.commit_time)
    }
}

impl std::cmp::PartialOrd for Key {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.commit_time.cmp(&other.commit_time))
    }
}

// Git's priority queue works as a LIFO stack if no compare function is set,
// which is the case for --topo-order
enum Queue {
    Date(PriorityQueue<i64, ObjectId>),
    Topo(Vec<ObjectId>),
}

impl Queue {
    fn new(s: Sorting) -> Self {
        match s {
            Sorting::DateOrder => Self::Date(PriorityQueue::new()),
            Sorting::TopoOrder => Self::Topo(vec![]),
        }
    }

    fn push(&mut self, commit_time: i64, id: ObjectId) {
        match self {
            Self::Date(q) => q.insert(commit_time, id),
            Self::Topo(q) => q.push(id),
        }
    }

    fn pop(&mut self) -> Option<ObjectId> {
        match self {
            Self::Date(q) => q.pop().map(|(_, id)| id),
            Self::Topo(q) => q.pop(),
        }
    }

    fn reverse(&mut self) {
        if let Queue::Topo(q) = self {
            q.reverse();
        }
    }
}

// #[derive(Debug)]
type WalkState = FlagSet<WalkFlags>;

/// A commit walker that walks in topographical order, like `git rev-list
/// --topo-order`. It requires a commit graph to be available, but not
/// necessarily up to date.
pub struct Walk<Find, E>
where
    Find: for<'a> Fn(&gix_hash::oid, &'a mut Vec<u8>) -> Result<gix_object::CommitRefIter<'a>, E>,
    E: std::error::Error + Send + Sync + 'static,
{
    commit_graph: Graph,
    find: Find,
    indegrees: IdMap<i32>,
    states: IdMap<WalkState>,
    explore_queue: PriorityQueue<u32, ObjectId>,
    indegree_queue: PriorityQueue<u32, ObjectId>,
    topo_queue: Queue,
    min_gen: u32,
    buf: Vec<u8>,
}

// #[trace(disable(new), prefix_enter = "", prefix_exit = "")]
impl<Find, E> Walk<Find, E>
where
    Find: for<'a> Fn(&gix_hash::oid, &'a mut Vec<u8>) -> Result<gix_object::CommitRefIter<'a>, E>,
    E: std::error::Error + Send + Sync + 'static,
{
    /// Create a new Walk that walks the given repository, starting at the
    /// tips and ending at the bottoms. Like `git rev-list --topo-order
    /// ^bottom... tips...`
    pub fn new(
        commit_graph: gix_commitgraph::Graph,
        f: Find,
        sorting: Sorting,
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
            topo_queue: Queue::new(sorting),
            min_gen: gix_commitgraph::GENERATION_NUMBER_INFINITY,
            buf: vec![],
        };

        let tips = tips.into_iter().map(Into::into).collect::<Vec<_>>();
        let tip_flags: FlagSet<WalkFlags> = WalkFlags::Seen.into();

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

            let commit = find(Some(&s.commit_graph), &s.find, id, &mut s.buf)
                .map_err(|_err| Error::CommitNotFound)?;

            let gen = match commit {
                Either::CommitRefIter(c) => gix_commitgraph::GENERATION_NUMBER_INFINITY,
                Either::CachedCommit(c) => c.generation(),
            };

            if gen < s.min_gen {
                s.min_gen = gen;
            }

            let state = flags | WalkFlags::Explored | WalkFlags::InDegree;

            s.states.insert(*id, state);
            s.explore_queue.insert(gen, *id);
            s.indegree_queue.insert(gen, *id);
        }

        // NOTE: Parents of ends must also be marked uninteresting for some
        // reason. See handle_commit()
        for id in &ends {
            let parents = s.collect_parents(id)?;
            for (id, _, _) in parents {
                s.states
                    .entry(id)
                    .and_modify(|s| *s |= WalkFlags::Uninteresting)
                    .or_insert(WalkFlags::Uninteresting | WalkFlags::Seen);
            }
        }

        s.compute_indegrees_to_depth(s.min_gen)?;

        for id in tips.iter() {
            let i = *s.indegrees.get(id).ok_or(Error::MissingIndegree)?;

            // NOTE: in Git the ends are also added to the topo_queue, but then
            // in simplify_commit() Git is told to ignore it. For now the tests pass.
            if i == 1 {
                let commit = find(Some(&s.commit_graph), &s.find, id, &mut s.buf)
                    .map_err(|_err| Error::CommitNotFound)?;

                let (gen, time) = match commit {
                    Either::CommitRefIter(c) => {
                        let mut commit_time = 0;
                        for token in c {
                            match token {
                                Ok(gix_object::commit::ref_iter::Token::Tree { .. }) => continue,
                                Ok(gix_object::commit::ref_iter::Token::Parent { .. }) => continue,
                                Ok(gix_object::commit::ref_iter::Token::Committer {
                                    signature,
                                }) => {
                                    commit_time = signature.time.seconds;
                                    break;
                                }
                                Ok(_unused_token) => break,
                                Err(err) => return Err(err.into()),
                            }
                        }
                        (gix_commitgraph::GENERATION_NUMBER_INFINITY, commit_time)
                    }
                    Either::CachedCommit(c) => (c.generation(), c.committer_timestamp() as i64),
                };

                s.topo_queue.push(time, *id);
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

            let parents = self.collect_parents(&id)?;

            for (id, gen, _) in parents {
                self.indegrees
                    .entry(id)
                    .and_modify(|e| *e += 1)
                    .or_insert(2);

                let state = self.states.get_mut(&id).ok_or(Error::MissingState)?;

                if !state.contains(WalkFlags::InDegree) {
                    *state |= WalkFlags::InDegree;
                    self.indegree_queue.insert(gen, id);
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
            let parents = self.collect_parents(&id)?;

            self.process_parents(&id, &parents)?;

            for (id, gen, _) in parents {
                let state = self.states.get_mut(&id).ok_or(Error::MissingState)?;

                if !state.contains(WalkFlags::Explored) {
                    *state |= WalkFlags::Explored;
                    self.explore_queue.insert(gen, id);
                }
            }
        }

        Ok(())
    }

    fn expand_topo_walk(&mut self, id: &oid) -> Result<(), Error> {
        let parents = self.collect_parents(id)?;

        self.process_parents(id, &parents)?;

        for (pid, parent_gen, parent_commit_time) in parents {
            let parent_state = self.states.get(&pid).ok_or(Error::MissingState)?;

            if parent_state.contains(WalkFlags::Uninteresting) {
                continue;
            }

            if parent_gen < self.min_gen {
                self.min_gen = parent_gen;
                self.compute_indegrees_to_depth(self.min_gen)?;
            }

            let i = self.indegrees.get_mut(&pid).ok_or(Error::MissingIndegree)?;

            *i -= 1;

            if *i == 1 {
                self.topo_queue.push(parent_commit_time, pid);
            }
        }

        Ok(())
    }

    fn process_parents(&mut self, id: &oid, parents: &[(ObjectId, u32, i64)]) -> Result<(), Error> {
        let state = self.states.get_mut(id).ok_or(Error::MissingState)?;

        if state.contains(WalkFlags::Added) {
            return Ok(());
        }

        *state |= WalkFlags::Added;

        // If the current commit is uninteresting we pass that on to parents,
        // otherwise we pass SymmetricLeft and AncestryPath + Seen
        let (pass, insert) = if state.contains(WalkFlags::Uninteresting) {
            let flags = WalkFlags::Uninteresting.into();
            (flags, flags)
        } else {
            let flags = *state & (WalkFlags::SymmetricLeft | WalkFlags::AncestryPath);
            (flags, flags | WalkFlags::Seen)
        };

        for (id, _, _) in parents {
            self.states
                .entry(*id)
                .and_modify(|s| *s |= pass)
                .or_insert(insert);
        }

        Ok(())
    }

    fn collect_parents(&mut self, id: &oid) -> Result<SmallVec<[(ObjectId, u32, i64); 1]>, Error> {
        collect_parents(Some(&self.commit_graph), &self.find, id, &mut self.buf)
    }
}

// #[trace(prefix_enter = "", prefix_exit = "")]
impl<Find, E> Iterator for Walk<Find, E>
where
    Find: for<'a> Fn(&gix_hash::oid, &'a mut Vec<u8>) -> Result<gix_object::CommitRefIter<'a>, E>,
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

pub struct Walk2 {
    list: Vec<ObjectId>,
}

// #[trace(disable(new), prefix_enter = "", prefix_exit = "")]
impl Walk2 {
    /// Create a new Walk that walks the given repository, starting at the
    /// tips and ending at the bottoms. Like `git rev-list --topo-order
    /// ^bottom... tips...`
    pub fn new<Find, E>(
        f: Find,
        tips: impl IntoIterator<Item = impl Into<ObjectId>>,
        ends: Option<impl IntoIterator<Item = impl Into<ObjectId>>>,
    ) -> Result<Self, Error>
    where
        Find:
            for<'a> Fn(&gix_hash::oid, &'a mut Vec<u8>) -> Result<gix_object::CommitRefIter<'a>, E>,
        E: std::error::Error + Send + Sync + 'static,
    {
        let a = gix_traverse::commit::Ancestors::new(
            tips,
            gix_traverse::commit::ancestors::State::default(),
            f,
        )
        .sorting(gix_traverse::commit::Sorting::ByCommitTimeNewestFirst)
        .expect("new sorting");

        let all = a.collect::<Result<Vec<_>, _>>()?;

        let mut indegrees = IdMap::from_iter(all.iter().map(|info| (info.id, 1)));
        let infos = IdMap::from_iter(all.iter().map(|info| (info.id, info.clone())));

        for info in &all {
            for parent in &info.parent_ids {
                indegrees.entry(*parent).and_modify(|i| *i += 1);
            }
        }

        let mut queue = PriorityQueue::<Key, ObjectId>::new();

        for info in all.iter().filter(|info| indegrees[&info.id] == 1) {
            queue.insert(Key::new(info.commit_time.expect("commit_time")), info.id);
        }

        let sort_by_time = true;
        if !sort_by_time {
            // Reverse queue
        }

        let mut final_list = vec![];

        while let Some((_, id)) = queue.pop() {
            let info = &infos[&id];
            for parent in &info.parent_ids {
                let i = indegrees.get_mut(parent).expect("indegrees.get_mut");

                if *i == 0 {
                    continue;
                }

                *i -= 1;

                if *i == 1 {
                    queue.insert(Key::new(info.commit_time.expect("commit_time 2")), *parent);
                }
            }

            *indegrees.get_mut(&id).expect("indegrees.get_mut 2") = 0;

            final_list.push(id);
        }

        final_list.reverse();

        Ok(Self { list: final_list })
    }
}

impl Iterator for Walk2 {
    type Item = Result<ObjectId, Error>;

    fn next(&mut self) -> Option<Self::Item> {
        self.list.pop().map(|id| Ok(id))
    }
}

enum Either<'buf, 'cache> {
    CommitRefIter(gix_object::CommitRefIter<'buf>),
    CachedCommit(gix_commitgraph::file::Commit<'cache>),
}

fn find<'cache, 'buf, Find, E>(
    cache: Option<&'cache gix_commitgraph::Graph>,
    find: Find,
    id: &oid,
    buf: &'buf mut Vec<u8>,
) -> Result<Either<'buf, 'cache>, E>
where
    Find: for<'a> Fn(&gix_hash::oid, &'a mut Vec<u8>) -> Result<gix_object::CommitRefIter<'a>, E>,
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
) -> Result<SmallVec<[(ObjectId, u32, i64); 1]>, Error>
where
    Find: for<'a> Fn(&oid, &'a mut Vec<u8>) -> Result<gix_object::CommitRefIter<'a>, E>,
    E: std::error::Error + Send + Sync + 'static,
{
    let mut parents = SmallVec::<[(ObjectId, u32, i64); 1]>::new();

    match find(cache, &f, id, buf).map_err(|err| Error::CommitNotFound)? {
        Either::CommitRefIter(c) => {
            for token in c {
                match token {
                    Ok(gix_object::commit::ref_iter::Token::Tree { .. }) => continue,
                    Ok(gix_object::commit::ref_iter::Token::Parent { id }) => {
                        parents.push((id, 0, 0)); // Dummy numbers to be filled in
                    }
                    Ok(_past_parents) => break,
                    Err(err) => return Err(err.into()),
                }
            }
            // Need to check the cache again. That a commit is not in the cache
            // doesn't mean a parent is not.
            for (id, gen, time) in parents.iter_mut() {
                (*gen, *time) = match find(cache, &f, id, buf)
                    .map_err(|err| Error::CommitNotFound)?
                {
                    Either::CommitRefIter(c) => {
                        let mut commit_time = 0;
                        for token in c {
                            match token {
                                Ok(gix_object::commit::ref_iter::Token::Tree { .. }) => continue,
                                Ok(gix_object::commit::ref_iter::Token::Parent { .. }) => continue,
                                Ok(gix_object::commit::ref_iter::Token::Committer {
                                    signature,
                                }) => {
                                    commit_time = signature.time.seconds;
                                    break;
                                }
                                Ok(_unused_token) => break,
                                Err(err) => return Err(err.into()),
                            }
                        }
                        (gix_commitgraph::GENERATION_NUMBER_INFINITY, commit_time)
                    }
                    Either::CachedCommit(c) => (c.generation(), c.committer_timestamp() as i64),
                };
            }
        }
        Either::CachedCommit(c) => {
            for pos in c.iter_parents() {
                let parent_commit = cache
                    .expect("cache exists if CachedCommit was returned")
                    .commit_at(pos?);
                parents.push((
                    parent_commit.id().into(),
                    parent_commit.generation(),
                    parent_commit.committer_timestamp() as i64,
                ));
            }
        }
    };

    Ok(parents)
}

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use super::*;
    use pretty_assertions::assert_eq;

    fn resolve(repo: &gix::Repository, range: &str) -> (ObjectId, Option<ObjectId>) {
        let range = repo.rev_parse(range).expect("valid range").detach();

        use gix_revision::Spec;

        match range {
            Spec::Include(oid) => (oid, None),
            Spec::Range { from, to } => (to, Some(from)),
            _ => panic!("lol"),
        }
    }

    fn compare<I, E>(iter: I, order_flag: &str, range: &str)
    where
        I: Iterator<Item = Result<gix::ObjectId, E>>,
        E: std::error::Error,
    {
        let mine = iter.collect::<Result<Vec<_>, _>>().unwrap();
        let fasit = git_rev_list(&[order_flag, range]);

        assert_eq!(
            &mine, &fasit,
            "left = mine, right = fasit, flag = {order_flag}",
        );
    }

    macro_rules! topo_test {
        // ($test_name:ident, $range:literal) => {};
        ($test_name:ident, $range:literal) => {
            #[test]
            fn $test_name() {
                let repo = gix::discover(".").unwrap();
                let (start, end) = resolve(&repo, $range);

                for (flag, sorting) in [
                    ("--date-order", Sorting::TopoOrder),
                    ("--topo-order", Sorting::TopoOrder),
                ] {
                    let walk = Walk::new(
                        repo.commit_graph().unwrap(),
                        |id, buf| repo.objects.find_commit_iter(id, buf),
                        sorting,
                        std::iter::once(start),
                        end.map(|e| std::iter::once(e)),
                    )
                    .unwrap();

                    compare(walk, flag, $range);
                }
            }
        };
    }

    macro_rules! topo_test2 {
        ($test_name:ident, $range:literal) => {};
        ($test_name:ident, $range:literal) => {
            #[test]
            fn $test_name() {
                let repo = gix::discover(".").unwrap();
                let (start, end) = resolve(&repo, $range);

                let walk = Walk2::new(
                    |id, buf| repo.objects.find_commit_iter(id, buf),
                    std::iter::once(start),
                    end.map(|e| std::iter::once(e)),
                )
                .unwrap();

                compare(walk, $range);
            }
        };
    }

    fn git_rev_list(args: &[&str]) -> Vec<gix::ObjectId> {
        let output = std::process::Command::new("git")
            .arg("rev-list")
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
    } // 753d1db: Initial commit
    topo_test!(t01_753d1db, "753d1db");

    // f28f649: Simple change
    topo_test!(t02_f28f649, "f28f649");
    topo_test!(t02_01_753d1db_f28f649, "753d1db..f28f649");

    // d3baed3: Removes more than it adds
    topo_test!(t03_d3baed3, "d3baed3");
    topo_test!(t03_01_753d1db_d3baed3, "753d1db..d3baed3");
    topo_test!(t03_02_f28f649_d3baed3, "f28f649..d3baed3");

    // 536a0f5: Adds more than it removes
    topo_test!(t04_536a0f5, "536a0f5");
    topo_test!(t04_01_753d1db_536a0f5, "753d1db..536a0f5");
    topo_test!(t04_02_f28f649_536a0f5, "f28f649..536a0f5");
    topo_test!(t04_03_d3baed3_536a0f5, "d3baed3..536a0f5");

    // 6a30c80: Change on first line
    topo_test!(t05_6a30c80, "6a30c80");
    topo_test!(t05_01_753d1db_6a30c80, "753d1db..6a30c80");
    topo_test!(t05_02_f28f649_6a30c80, "f28f649..6a30c80");
    topo_test!(t05_03_d3baed3_6a30c80, "d3baed3..6a30c80");
    topo_test!(t05_04_536a0f5_6a30c80, "536a0f5..6a30c80");

    // 4d8a3c7: Multiple changes in one commit
    topo_test!(t06_4d8a3c7, "4d8a3c7");
    topo_test!(t06_01_753d1db_4d8a3c7, "753d1db..4d8a3c7");
    topo_test!(t06_02_f28f649_4d8a3c7, "f28f649..4d8a3c7");
    topo_test!(t06_03_d3baed3_4d8a3c7, "d3baed3..4d8a3c7");
    topo_test!(t06_04_536a0f5_4d8a3c7, "536a0f5..4d8a3c7");
    topo_test!(t06_05_6a30c80_4d8a3c7, "6a30c80..4d8a3c7");

    // 2064b3c: Change on last line
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

    // 616867d: October
    topo_test!(t13_616867d, "616867d");
    topo_test!(t13_01_753d1db_616867d, "753d1db..616867d");
    topo_test!(t13_02_f28f649_616867d, "f28f649..616867d");
    topo_test!(t13_03_d3baed3_616867d, "d3baed3..616867d");
    topo_test!(t13_04_536a0f5_616867d, "536a0f5..616867d");
    topo_test!(t13_05_6a30c80_616867d, "6a30c80..616867d");
    topo_test!(t13_06_4d8a3c7_616867d, "4d8a3c7..616867d");
    topo_test!(t13_07_2064b3c_616867d, "2064b3c..616867d");
    topo_test!(t13_08_0e17ccb_616867d, "0e17ccb..616867d");
    topo_test!(t13_09_3be8265_616867d, "3be8265..616867d");
    topo_test!(t13_10_8bf8780_616867d, "8bf8780..616867d");
    topo_test!(t13_11_f7a3a57_616867d, "f7a3a57..616867d");
    topo_test!(t13_12_392db1b_616867d, "392db1b..616867d");

    // bb48275: Side project
    topo_test!(t14_bb48275, "bb48275");
    topo_test!(t14_01_753d1db_bb48275, "753d1db..bb48275");
    topo_test!(t14_02_f28f649_bb48275, "f28f649..bb48275");
    topo_test!(t14_03_d3baed3_bb48275, "d3baed3..bb48275");
    topo_test!(t14_04_536a0f5_bb48275, "536a0f5..bb48275");
    topo_test!(t14_05_6a30c80_bb48275, "6a30c80..bb48275");
    topo_test!(t14_06_4d8a3c7_bb48275, "4d8a3c7..bb48275");
    topo_test!(t14_07_2064b3c_bb48275, "2064b3c..bb48275");
    topo_test!(t14_08_0e17ccb_bb48275, "0e17ccb..bb48275");
    topo_test!(t14_09_3be8265_bb48275, "3be8265..bb48275");
    topo_test!(t14_10_8bf8780_bb48275, "8bf8780..bb48275");
    topo_test!(t14_11_f7a3a57_bb48275, "f7a3a57..bb48275");
    topo_test!(t14_12_392db1b_bb48275, "392db1b..bb48275");
    topo_test!(t14_13_616867d_bb48275, "616867d..bb48275");

    // bb8601c: Merge branch 'kek2' into HEAD
    topo_test!(t15_bb8601c, "bb8601c");
    topo_test!(t15_01_753d1db_bb8601c, "753d1db..bb8601c");
    topo_test!(t15_02_f28f649_bb8601c, "f28f649..bb8601c");
    topo_test!(t15_03_d3baed3_bb8601c, "d3baed3..bb8601c");
    topo_test!(t15_04_536a0f5_bb8601c, "536a0f5..bb8601c");
    topo_test!(t15_05_6a30c80_bb8601c, "6a30c80..bb8601c");
    topo_test!(t15_06_4d8a3c7_bb8601c, "4d8a3c7..bb8601c");
    topo_test!(t15_07_2064b3c_bb8601c, "2064b3c..bb8601c");
    topo_test!(t15_08_0e17ccb_bb8601c, "0e17ccb..bb8601c");
    topo_test!(t15_09_3be8265_bb8601c, "3be8265..bb8601c");
    topo_test!(t15_10_8bf8780_bb8601c, "8bf8780..bb8601c");
    topo_test!(t15_11_f7a3a57_bb8601c, "f7a3a57..bb8601c");
    topo_test!(t15_12_392db1b_bb8601c, "392db1b..bb8601c");
    topo_test!(t15_13_616867d_bb8601c, "616867d..bb8601c");
    topo_test!(t15_14_bb48275_bb8601c, "bb48275..bb8601c");

    // 00491e2: Multiple changes in one commit again
    topo_test!(t16_00491e2, "00491e2");
    topo_test!(t16_01_753d1db_00491e2, "753d1db..00491e2");
    topo_test!(t16_02_f28f649_00491e2, "f28f649..00491e2");
    topo_test!(t16_03_d3baed3_00491e2, "d3baed3..00491e2");
    topo_test!(t16_04_536a0f5_00491e2, "536a0f5..00491e2");
    topo_test!(t16_05_6a30c80_00491e2, "6a30c80..00491e2");
    topo_test!(t16_06_4d8a3c7_00491e2, "4d8a3c7..00491e2");
    topo_test!(t16_07_2064b3c_00491e2, "2064b3c..00491e2");
    topo_test!(t16_08_0e17ccb_00491e2, "0e17ccb..00491e2");
    topo_test!(t16_09_3be8265_00491e2, "3be8265..00491e2");
    topo_test!(t16_10_8bf8780_00491e2, "8bf8780..00491e2");
    topo_test!(t16_11_f7a3a57_00491e2, "f7a3a57..00491e2");
    topo_test!(t16_12_392db1b_00491e2, "392db1b..00491e2");
    topo_test!(t16_13_616867d_00491e2, "616867d..00491e2");
    topo_test!(t16_14_bb48275_00491e2, "bb48275..00491e2");
    topo_test!(t16_15_bb8601c_00491e2, "bb8601c..00491e2");
}
