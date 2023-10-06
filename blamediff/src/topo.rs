use gix_commitgraph::Graph;
use gix_hash::{oid, ObjectId};
use gix_hashtable::hash_map::Entry;
use gix_odb::FindExt;
use gix_revwalk::{graph::IdMap, PriorityQueue};

use flagset::{flags, FlagSet};

// use ::trace::trace;
// trace::init_depth_var!();

flags! {
    enum WalkFlags: u32 {
        Explored,
        InDegree,
        Uninteresting,
        Bottom,
        Added,
        SymmetricLeft,
        AncestryPath,
        Seen,
    }
}

#[derive(Debug)]
struct WalkState(FlagSet<WalkFlags>);

#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error("Calculated indegree missing")]
    MissingIndegree,
    #[error("Internal state not found")]
    MissingState,
    #[error("Commit not found in commit graph")]
    CommitNotFound,
    #[error("Error initializing graph: {0}")]
    CommitGraphInit(#[from] gix_commitgraph::init::Error),
    #[error("Error doing file stuff: {0}")]
    CommitGraphFile(#[from] gix_commitgraph::file::commit::Error),
}

/// A commit walker that walks in topographical order, like `git rev-list --topo-order`.
pub struct TopoWalker<'repo> {
    repo: &'repo gix::Repository,
    commit_graph: Graph,
    indegrees: IdMap<i32>,
    states: IdMap<WalkState>,
    explore_queue: PriorityQueue<u32, ObjectId>,
    indegree_queue: PriorityQueue<u32, ObjectId>,
    topo_queue: Vec<ObjectId>,
    min_gen: u32,
    buf: Vec<u8>,
}

// #[trace(disable(on_repo))]
impl<'repo> TopoWalker<'repo> {
    /// Create a new TopoWalker that walks the given repository, starting at the
    /// tips and ending at the bottoms. Like `git rev-list --topo-order
    /// ^bottom... tips...`
    pub fn on_repo(
        repo: &'repo gix::Repository,
        tips: impl IntoIterator<Item = impl Into<ObjectId>>,
        bottoms: impl IntoIterator<Item = impl Into<ObjectId>>,
    ) -> Result<Self, Error> {
        let mut s = Self {
            repo,
            commit_graph: repo.commit_graph()?,
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

        let bottom_flags = tip_flags | WalkFlags::Uninteresting | WalkFlags::Bottom;
        let bottoms = bottoms.into_iter().map(Into::into).collect::<Vec<_>>();

        for (id, flags) in tips
            .iter()
            .map(|id| (id, tip_flags))
            .chain(bottoms.iter().map(|id| (id, bottom_flags)))
        {
            *s.indegrees.entry(*id).or_default() = 1;

            let gen = match find(
                &s.commit_graph,
                |id, buf| s.repo.objects.find_commit_iter(id, buf),
                id,
                &mut s.buf,
            ) {
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

        for id in tips.iter().chain(bottoms.iter()) {
            let i = *s.indegrees.get(id).ok_or(Error::MissingIndegree)?;

            if i == 1 {
                dbg!(id);
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

            let pgen = get_parent_generations(
                &self.commit_graph,
                |id, buf| self.repo.objects.find_commit_iter(id, buf),
                &id,
                &mut self.buf,
            );

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
            self.process_parents(id)?;

            let pgen = get_parent_generations(
                &self.commit_graph,
                |id, buf| self.repo.objects.find_commit_iter(id, buf),
                &id,
                &mut self.buf,
            );

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

    fn expand_topo_walk(&mut self, id: ObjectId) -> Result<(), Error> {
        self.process_parents(id)?;

        let pgen = get_parent_generations(
            &self.commit_graph,
            |id, buf| self.repo.objects.find_commit_iter(id, buf),
            &id,
            &mut self.buf,
        );

        for (pid, parent_gen) in pgen {
            let parent_flags = self.states.get(&pid).ok_or(Error::MissingState)?;

            if parent_flags.0.contains(WalkFlags::Uninteresting) {
                continue;
            }

            if parent_gen < self.min_gen {
                self.min_gen = parent_gen;
                self.compute_indegree_to_depth(self.min_gen)?;
            }

            let i = self.indegrees.get_mut(&pid).ok_or(Error::MissingIndegree)?;

            *i -= 1;

            if *i == 1 {
                // topodbg!(&pid);
                self.topo_queue.push(pid);
            }
        }

        Ok(())
    }

    fn process_parents(&mut self, id: ObjectId) -> Result<(), Error> {
        let state = self.states.get_mut(&id).ok_or(Error::MissingState)?;

        if state.0.contains(WalkFlags::Added) {
            return Ok(());
        }

        state.0 != WalkFlags::Added;

        let pass_flags = if state.0.contains(WalkFlags::Uninteresting) {
            state.0 & (WalkFlags::SymmetricLeft | WalkFlags::AncestryPath)
        } else {
            state.0 & WalkFlags::Uninteresting
        };

        let pgen = get_parent_generations(
            &self.commit_graph,
            |id, buf| self.repo.objects.find_commit_iter(id, buf),
            &id,
            &mut self.buf,
        );

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

fn get_parent_generations<'b, 'g, Find, E>(
    commit_graph: &'g gix_commitgraph::Graph,
    f: Find,
    id: &oid,
    buf: &'b mut Vec<u8>,
) -> Vec<(ObjectId, u32)>
where
    Find:
        for<'a> FnMut(&gix_hash::oid, &'a mut Vec<u8>) -> Result<gix_object::CommitRefIter<'a>, E>,
    E: std::error::Error + Send + Sync + 'static,
{
    let mut pgen = vec![];

    match find(commit_graph, f, &id, buf) {
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
                let parent_commit = commit_graph.commit_at(p.expect("get position"));
                let pid = ObjectId::from(parent_commit.id());
                pgen.push((pid, parent_commit.generation()));
            }
        }
    };

    pgen
}

fn find<'b, 'g, Find, E>(
    commit_graph: &'g gix_commitgraph::Graph,
    mut find: Find,
    id: &oid,
    buf: &'b mut Vec<u8>,
) -> Either<'b, 'g>
where
    Find:
        for<'a> FnMut(&gix_hash::oid, &'a mut Vec<u8>) -> Result<gix_object::CommitRefIter<'a>, E>,
    E: std::error::Error + Send + Sync + 'static,
{
    match commit_graph.commit_by_id(id).map(Either::CachedCommit) {
        Some(c) => c,
        None => (find)(id, buf).map(Either::CommitRefIter).unwrap(),
    }
}

// #[trace]
impl Iterator for TopoWalker<'_> {
    type Item = Result<ObjectId, Error>;

    fn next(&mut self) -> Option<Self::Item> {
        let id = self.topo_queue.pop()?;

        let i = match self.indegrees.get_mut(&id) {
            Some(i) => i,
            None => {
                return Some(Err(Error::MissingIndegree));
            }
        };
        // .map_err(|e| Some(e))?;
        *i = 0;

        match self.expand_topo_walk(id) {
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
        let t = TopoWalker::on_repo(
            &r,
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
        let t = TopoWalker::on_repo(
            &r,
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
        let bottom = r.rev_parse_single("6a30c80").unwrap();
        let t = TopoWalker::on_repo(&r, std::iter::once(tip), std::iter::once(bottom)).unwrap();

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
        let bottom = r.rev_parse_single("6a30c80").unwrap();
        let t = TopoWalker::on_repo(&r, std::iter::once(tip), std::iter::once(bottom)).unwrap();

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
        let bottom = r.rev_parse_single("8bf8780").unwrap();
        let t = TopoWalker::on_repo(&r, std::iter::once(tip), std::iter::once(bottom)).unwrap();

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
        let bottom = r.rev_parse_single("bb48275").unwrap();
        let t = TopoWalker::on_repo(&r, std::iter::once(tip), std::iter::once(bottom)).unwrap();

        let mine = t
            .map(|id| id.unwrap().to_hex().to_string())
            .collect::<Vec<_>>();
        let fasit = run_git_rev_list(&["bb48275..second-test"]);
        assert_eq!(mine, fasit);
    }
}
