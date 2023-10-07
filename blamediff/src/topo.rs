use gix_commitgraph::Graph;
use gix_hash::{oid, ObjectId};
use gix_hashtable::hash_map::Entry;
use gix_odb::FindExt;
use gix_revwalk::{graph::IdMap, PriorityQueue};

use flagset::{flags, FlagSet};

use smallvec::SmallVec;

// use ::trace::trace;
// trace::init_depth_var!();

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
// pub struct TopoWalker<'repo> {
pub struct TopoWalker<Find, E>
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

// #[trace(disable(on_repo))]
impl<Find, E> TopoWalker<Find, E>
where
    Find:
        for<'a> FnMut(&gix_hash::oid, &'a mut Vec<u8>) -> Result<gix_object::CommitRefIter<'a>, E>,
    E: std::error::Error + Send + Sync + 'static,
{
    /// Create a new TopoWalker that walks the given repository, starting at the
    /// tips and ending at the bottoms. Like `git rev-list --topo-order
    /// ^bottom... tips...`
    pub fn new(
        commit_graph: gix_commitgraph::Graph,
        find: Find,
        tips: impl IntoIterator<Item = impl Into<ObjectId>>,
        ends: impl IntoIterator<Item = impl Into<ObjectId>>,
    ) -> Result<Self, Error> {
        let mut s = Self {
            commit_graph,
            find,
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
        let ends = ends.into_iter().map(Into::into).collect::<Vec<_>>();

        for (id, flags) in tips
            .iter()
            .map(|id| (id, tip_flags))
            .chain(ends.iter().map(|id| (id, end_flags)))
        {
            *s.indegrees.entry(*id).or_default() = 1;

            let commit = crate::topo::find(&s.commit_graph, &mut s.find, id, &mut s.buf)
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

        s.compute_indegree_to_depth(s.min_gen)?;

        for id in tips.iter().chain(ends.iter()) {
            let i = *s.indegrees.get(id).ok_or(Error::MissingIndegree)?;

            if i == 1 {
                s.topo_queue.push(*id);
            }
        }

        s.topo_queue.reverse();

        Ok(s)
    }

    fn compute_indegree_to_depth(&mut self, gen_cutoff: u32) -> Result<(), Error> {
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
                get_parent_generations(&self.commit_graph, &mut self.find, &id, &mut self.buf)?;

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
                get_parent_generations(&self.commit_graph, &mut self.find, &id, &mut self.buf)?;

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
        self.process_parents(id)?;

        let pgen = get_parent_generations(&self.commit_graph, &mut self.find, &id, &mut self.buf)?;

        for (pid, parent_gen) in pgen {
            let parent_state = self.states.get(&pid).ok_or(Error::MissingState)?;

            if parent_state.0.contains(WalkFlags::Uninteresting) {
                continue;
            }

            if parent_gen < self.min_gen {
                self.min_gen = parent_gen;
                self.compute_indegree_to_depth(self.min_gen)?;
            }

            let i = self.indegrees.get_mut(&pid).ok_or(Error::MissingIndegree)?;

            *i -= 1;

            if *i == 1 {
                self.topo_queue.push(pid);
            }
        }

        Ok(())
    }

    fn process_parents(&mut self, id: &oid) -> Result<(), Error> {
        let state = self.states.get_mut(id).ok_or(Error::MissingState)?;

        if state.0.contains(WalkFlags::Added) {
            return Ok(());
        }

        state.0 != WalkFlags::Added;

        let pass_flags = if !state.0.contains(WalkFlags::Uninteresting) {
            state.0 & (WalkFlags::SymmetricLeft | WalkFlags::AncestryPath)
        } else {
            state.0
        };

        let pgen = get_parent_generations(&self.commit_graph, &mut self.find, &id, &mut self.buf)?;

        for (pid, _) in pgen {
            match self.states.entry(pid) {
                Entry::Occupied(mut o) => o.get_mut().0 |= pass_flags,
                Entry::Vacant(v) => {
                    v.insert(WalkState(pass_flags));
                }
            };
        }

        Ok(())
    }
}

// #[trace]
impl<Find, E> Iterator for TopoWalker<Find, E>
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

fn find<'b, 'g, Find, E>(
    commit_graph: &'g gix_commitgraph::Graph,
    mut find: Find,
    id: &oid,
    buf: &'b mut Vec<u8>,
) -> Result<Either<'b, 'g>, E>
where
    Find:
        for<'a> FnMut(&gix_hash::oid, &'a mut Vec<u8>) -> Result<gix_object::CommitRefIter<'a>, E>,
    E: std::error::Error + Send + Sync + 'static,
{
    match commit_graph.commit_by_id(id).map(Either::CachedCommit) {
        Some(c) => Ok(c),
        None => (find)(id, buf).map(Either::CommitRefIter),
    }
}

fn get_parent_generations<'b, 'g, Find, E>(
    commit_graph: &'g gix_commitgraph::Graph,
    f: Find,
    id: &oid,
    buf: &'b mut Vec<u8>,
) -> Result<SmallVec<[(ObjectId, u32); 1]>, Error>
where
    Find: for<'a> FnMut(&oid, &'a mut Vec<u8>) -> Result<gix_object::CommitRefIter<'a>, E>,
    E: std::error::Error + Send + Sync + 'static,
{
    let mut pgen = SmallVec::new();

    match find(commit_graph, f, &id, buf).map_err(|err| Error::CommitNotFound)? {
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
                let parent_commit = commit_graph.commit_at(p?);
                let pid = ObjectId::from(parent_commit.id());
                pgen.push((pid, parent_commit.generation()));
            }
        }
    };

    Ok(pgen)
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    fn run_git_rev_list(args: &[&str]) -> Vec<String> {
        let output = std::process::Command::new("git")
            .args(["rev-list", "--topo-order"])
            .args(args)
            .output()
            .expect("able to run git rev-list")
            .stdout;

        let output = std::str::from_utf8(&output).expect("valid UTF-8");

        output.split_terminator('\n').map(String::from).collect()
    }

    #[test]
    fn first_test() {
        let r = gix::discover(".").unwrap();
        let tip = r.rev_parse_single("first-test").unwrap();
        let t = TopoWalker::new(
            r.commit_graph().unwrap(),
            |id, buf| r.objects.find_commit_iter(id, buf),
            std::iter::once(tip),
            std::iter::empty::<gix::ObjectId>(),
        )
        .unwrap();

        let mine = t
            .map(|id| id.unwrap().to_hex().to_string())
            .collect::<Vec<_>>();
        let fasit = run_git_rev_list(&["first-test"]);
        assert_eq!(mine, fasit);
    }

    #[test]
    fn second_test() {
        let r = gix::discover(".").unwrap();
        let tip = r.rev_parse_single("second-test").unwrap();
        let t = TopoWalker::new(
            r.commit_graph().unwrap(),
            |id, buf| r.objects.find_commit_iter(id, buf),
            std::iter::once(tip),
            std::iter::empty::<gix::ObjectId>(),
        )
        .unwrap();

        let mine = t
            .map(|id| id.unwrap().to_hex().to_string())
            .collect::<Vec<_>>();
        let fasit = run_git_rev_list(&["second-test"]);
        assert_eq!(mine, fasit);
    }

    #[test]
    fn first_limited_test() {
        let r = gix::discover(".").unwrap();
        let tip = r.rev_parse_single("first-test").unwrap();
        let end = r.rev_parse_single("6a30c80").unwrap();
        let t = TopoWalker::new(
            r.commit_graph().unwrap(),
            |id, buf| r.objects.find_commit_iter(id, buf),
            std::iter::once(tip),
            std::iter::once(end),
        )
        .unwrap();
        let mine = t
            .map(|id| id.unwrap().to_hex().to_string())
            .collect::<Vec<_>>();
        let fasit = run_git_rev_list(&["6a30c80..first-test"]);
        assert_eq!(mine, fasit);
    }

    #[test]
    fn second_limited_test() {
        let r = gix::discover(".").unwrap();
        let tip = r.rev_parse_single("second-test").unwrap();
        let end = r.rev_parse_single("6a30c80").unwrap();
        let t = TopoWalker::new(
            r.commit_graph().unwrap(),
            |id, buf| r.objects.find_commit_iter(id, buf),
            std::iter::once(tip),
            std::iter::once(end),
        )
        .unwrap();

        let mine = t
            .map(|id| id.unwrap().to_hex().to_string())
            .collect::<Vec<_>>();
        let fasit = run_git_rev_list(&["6a30c80..second-test"]);
        assert_eq!(mine, fasit);
    }

    #[test]
    fn second_limited_test_left() {
        let r = gix::discover(".").unwrap();
        let tip = r.rev_parse_single("second-test").unwrap();
        let end = r.rev_parse_single("8bf8780").unwrap();
        let t = TopoWalker::new(
            r.commit_graph().unwrap(),
            |id, buf| r.objects.find_commit_iter(id, buf),
            std::iter::once(tip),
            std::iter::once(end),
        )
        .unwrap();

        let mine = t
            .map(|id| id.unwrap().to_hex().to_string())
            .collect::<Vec<_>>();
        let fasit = run_git_rev_list(&["8bf8780..second-test"]);
        assert_eq!(mine, fasit);
    }

    #[test]
    fn second_limited_test_right() {
        let r = gix::discover(".").unwrap();
        let tip = r.rev_parse_single("second-test").unwrap();
        let end = r.rev_parse_single("bb48275").unwrap();
        let t = TopoWalker::new(
            r.commit_graph().unwrap(),
            |id, buf| r.objects.find_commit_iter(id, buf),
            std::iter::once(tip),
            std::iter::once(end),
        )
        .unwrap();

        let mine = t
            .map(|id| id.unwrap().to_hex().to_string())
            .collect::<Vec<_>>();
        let fasit = run_git_rev_list(&["bb48275..second-test"]);
        assert_eq!(mine, fasit);
    }
}
