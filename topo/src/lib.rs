use gix_hash::{oid, ObjectId};
#[cfg(feature = "standalone")]
use gix_object::FindExt;
use gix_revwalk::{graph::IdMap, PriorityQueue};

use flagset::{flags, FlagSet};

use smallvec::SmallVec;

#[derive(thiserror::Error, Debug)]
#[allow(missing_docs)]
pub enum Error {
    #[error("Calculated indegree missing")]
    MissingIndegree,
    #[error("Internal state not found")]
    MissingState,
    #[error("Error initializing graph: {0}")]
    CommitGraphInit(#[from] gix_commitgraph::init::Error),
    #[error("Error doing file stuff: {0}")]
    CommitGraphFile(#[from] gix_commitgraph::file::commit::Error),
    #[error("Error decoding stuff: {0}")]
    ObjectDecode(#[from] gix_object::decode::Error),
    #[error("Error finding object: {0}")]
    Find(#[from] gix_object::find::existing_iter::Error),
}

#[cfg(feature = "trace")]
use ::trace::trace;
#[cfg(feature = "trace")]
trace::init_depth_var!();

flags! {
    /// Set of flags to describe the state of a particular commit while iterating.
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
        /// TODO: Figure out purpose of this flag
        Added,
        /// TODO: Figure out purpose of this flag
        SymmetricLeft,
        /// TODO: Figure out purpose of this flag
        AncestryPath,
    }
}

/// Sorting to use for the topological walk
#[derive(Clone, Copy, Debug, Default)]
pub enum Sorting {
    /// Show no parents before all of its children are shown, but otherwise show
    /// commits in the commit timestamp order.
    #[default]
    DateOrder,

    /// Show no parents before all of its children are shown, and avoid
    /// showing commits on multiple lines of history intermixed.
    TopoOrder,
}

/// Specify how to handle commit parents during traversal.
#[derive(Clone, Copy, Debug, Default)]
pub enum Parents {
    /// Traverse all parents, useful for traversing the entire ancestry.
    #[default]
    All,

    ///Only traverse along the first parent, which commonly ignores all branches.
    First,
}
/// Information about a commit that we obtained naturally as part of the iteration.
#[derive(Debug, PartialEq)]
pub struct Info {
    /// The id of the commit.
    pub id: ObjectId,
    /// All parent ids we have encountered. Note that these will be at most one if [`Parents::First`] is enabled.
    pub parent_ids: SmallVec<[ObjectId; 1]>,
}

// Git's priority queue works as a LIFO stack if no compare function is set,
// which is the case for --topo-order
enum Queue {
    Date(PriorityQueue<i64, Info>),
    Topo(Vec<Info>),
}

#[cfg_attr(
    feature = "trace",
    trace(disable(new), prefix_enter = "", prefix_exit = "")
)]
impl Queue {
    fn new(s: Sorting) -> Self {
        match s {
            Sorting::DateOrder => Self::Date(PriorityQueue::new()),
            Sorting::TopoOrder => Self::Topo(vec![]),
        }
    }

    fn push(&mut self, commit_time: i64, id: Info) {
        match self {
            Self::Date(q) => q.insert(commit_time, id),
            Self::Topo(q) => q.push(id),
        }
    }

    fn pop(&mut self) -> Option<Info> {
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

type GenAndCommitTime = (u32, i64);

/// Builder for `Walk`
pub struct Builder<Find, Predicate> {
    commit_graph: Option<gix_commitgraph::Graph>,
    find: Find,
    predicate: Predicate,
    sorting: Sorting,
    parents: Parents,
    tips: Vec<ObjectId>,
    ends: Vec<ObjectId>,
}

impl<Find> Builder<Find, fn(&oid) -> bool>
where
    Find: gix_object::Find,
{
    /// Create a new Builder for a Walk that walks the given repository, starting at the
    /// tips and ending at the ends. Like `git rev-list --topo-order
    /// ^ends... tips...`
    pub fn from_iters(
        find: Find,
        tips: impl IntoIterator<Item = impl Into<ObjectId>>,
        ends: Option<impl IntoIterator<Item = impl Into<ObjectId>>>,
    ) -> Self {
        let tips = tips.into_iter().map(Into::into).collect::<Vec<_>>();
        let ends = ends
            .map(|e| e.into_iter().map(Into::into).collect::<Vec<_>>())
            .unwrap_or_default();

        Self {
            commit_graph: Default::default(),
            find,
            sorting: Default::default(),
            parents: Default::default(),
            tips,
            ends,
            predicate: |_| true,
        }
    }

    /// Create a new Builder for a Walk that walks the given repository from an
    /// iterator of `Specs`, given by the `[gix_revision::Spec]Specs`
    pub fn from_specs(find: Find, specs: impl IntoIterator<Item = gix_revision::Spec>) -> Self {
        let mut tips = vec![];
        let mut ends = vec![];

        for spec in specs {
            use gix_revision::Spec as S;
            match spec {
                S::Include(i) => tips.push(i),
                S::Exclude(e) => ends.push(e),
                S::Range { from, to } => {
                    tips.push(to);
                    ends.push(from)
                }
                S::Merge { .. } => todo!(),
                S::IncludeOnlyParents(_) => todo!(),
                S::ExcludeParents(_) => todo!(),
            }
        }

        Self {
            commit_graph: Default::default(),
            find,
            sorting: Default::default(),
            parents: Default::default(),
            tips,
            ends,
            predicate: |_| true,
        }
    }

    /// Set a predicate to filter out revisions from the walk. Can be used to
    /// implement e.g. filtering on paths or time. This does *not* exclude the
    /// parent(s) of a revision that is excluded.
    pub fn with_predicate<Predicate>(self, predicate: Predicate) -> Builder<Find, Predicate>
    where
        Predicate: FnMut(&oid) -> bool,
    {
        Builder {
            commit_graph: self.commit_graph,
            find: self.find,
            sorting: self.sorting,
            parents: self.parents,
            tips: self.tips,
            ends: self.ends,
            predicate,
        }
    }
}
impl<Find, Predicate> Builder<Find, Predicate>
where
    Find: gix_object::Find,
    Predicate: FnMut(&oid) -> bool,
{
    /// Set the [`Sorting`] to use for the topological walk
    pub fn sorting(mut self, sorting: Sorting) -> Self {
        self.sorting = sorting;
        self
    }

    /// Specify how to handle commit parents during traversal.
    pub fn parents(mut self, parents: Parents) -> Self {
        self.parents = parents;
        self
    }

    /// Set or unset the commit-graph to use for the iteration.
    pub fn with_commit_graph(mut self, commit_graph: Option<gix_commitgraph::Graph>) -> Self {
        self.commit_graph = commit_graph;
        self
    }

    /// Build a new [`Walk`] instance.
    pub fn build(self) -> Result<Walk<Find, Predicate>, Error> {
        Walk::new(
            self.commit_graph,
            self.find,
            self.sorting,
            self.parents,
            &self.tips,
            &self.ends,
            self.predicate,
        )
    }
}

/// A commit walker that walks in topographical order, like `git rev-list
/// --topo-order` or `--date-order` depending on the chosen [`Sorting`]
pub struct Walk<Find, Predicate> {
    commit_graph: Option<gix_commitgraph::Graph>,
    find: Find,
    predicate: Predicate,
    indegrees: IdMap<i32>,
    states: IdMap<FlagSet<WalkFlags>>,
    explore_queue: PriorityQueue<GenAndCommitTime, ObjectId>,
    indegree_queue: PriorityQueue<GenAndCommitTime, ObjectId>,
    topo_queue: Queue,
    parents: Parents,
    min_gen: u32,
    buf: Vec<u8>,
}

impl<Find, Predicate> Walk<Find, Predicate>
where
    Find: gix_object::Find,
    Predicate: FnMut(&oid) -> bool,
{
    /// Create a new Walk that walks the given repository, starting at the
    /// tips and ending at the bottoms. Like `git rev-list --topo-order
    /// ^bottom... tips...`
    fn new(
        commit_graph: Option<gix_commitgraph::Graph>,
        f: Find,
        sorting: Sorting,
        parents: Parents,
        tips: &[ObjectId],
        ends: &[ObjectId],
        predicate: Predicate,
    ) -> Result<Self, Error> {
        let mut s = Self {
            commit_graph,
            find: f,
            predicate,
            indegrees: IdMap::default(),
            states: IdMap::default(),
            explore_queue: PriorityQueue::new(),
            indegree_queue: PriorityQueue::new(),
            topo_queue: Queue::new(sorting),
            parents,
            min_gen: gix_commitgraph::GENERATION_NUMBER_INFINITY,
            buf: vec![],
        };

        s.init(tips, ends)?;

        Ok(s)
    }
}

#[cfg_attr(feature = "trace", trace(prefix_enter = "", prefix_exit = ""))]
impl<Find, Predicate> Walk<Find, Predicate>
where
    Find: gix_object::Find,
{
    fn init(&mut self, tips: &[ObjectId], ends: &[ObjectId]) -> Result<(), Error> {
        let tip_flags: FlagSet<WalkFlags> = WalkFlags::Seen.into();
        let end_flags = tip_flags | WalkFlags::Uninteresting | WalkFlags::Bottom;

        for (id, flags) in tips
            .iter()
            .map(|id| (id, tip_flags))
            .chain(ends.iter().map(|id| (id, end_flags)))
        {
            *self.indegrees.entry(*id).or_default() = 1;

            let commit = find(self.commit_graph.as_ref(), &self.find, id, &mut self.buf)?;

            let (gen, time) = get_gen_and_commit_time(commit)?;

            if gen < self.min_gen {
                self.min_gen = gen;
            }

            let state = flags | WalkFlags::Explored | WalkFlags::InDegree;

            self.states.insert(*id, state);
            self.explore_queue.insert((gen, time), *id);
            self.indegree_queue.insert((gen, time), *id);
        }

        // NOTE: Parents of ends must also be marked uninteresting for some
        // reason. See handle_commit()
        for id in ends {
            let parents = self.collect_all_parents(id)?;
            for (id, _) in parents {
                self.states
                    .entry(id)
                    .and_modify(|s| *s |= WalkFlags::Uninteresting)
                    .or_insert(WalkFlags::Uninteresting | WalkFlags::Seen);
            }
        }

        self.compute_indegrees_to_depth(self.min_gen)?;

        for id in tips.iter() {
            let i = *self.indegrees.get(id).ok_or(Error::MissingIndegree)?;

            // NOTE: in Git the ends are also added to the topo_queue, but then
            // in simplify_commit() Git is told to ignore it. For now the tests pass.
            if i == 1 {
                let commit = find(self.commit_graph.as_ref(), &self.find, id, &mut self.buf)?;

                let (_, time) = get_gen_and_commit_time(commit)?;

                let parent_ids = self
                    .collect_all_parents(id)?
                    .into_iter()
                    .map(|e| e.0)
                    .collect();

                self.topo_queue.push(
                    time,
                    Info {
                        id: *id,
                        parent_ids,
                    },
                );
            }
        }

        self.topo_queue.reverse();

        Ok(())
    }

    fn compute_indegrees_to_depth(&mut self, gen_cutoff: u32) -> Result<(), Error> {
        while let Some(((gen, _), _)) = self.indegree_queue.peek() {
            if *gen >= gen_cutoff {
                self.indegree_walk_step()?;
            } else {
                break;
            }
        }

        Ok(())
    }

    fn indegree_walk_step(&mut self) -> Result<(), Error> {
        if let Some(((gen, _), id)) = self.indegree_queue.pop() {
            self.explore_to_depth(gen)?;

            let parents = self.collect_parents(&id)?;

            for (id, gen_time) in parents {
                self.indegrees
                    .entry(id)
                    .and_modify(|e| *e += 1)
                    .or_insert(2);

                let state = self.states.get_mut(&id).ok_or(Error::MissingState)?;

                if !state.contains(WalkFlags::InDegree) {
                    *state |= WalkFlags::InDegree;
                    self.indegree_queue.insert(gen_time, id);
                }
            }
        }

        Ok(())
    }

    fn explore_to_depth(&mut self, gen_cutoff: u32) -> Result<(), Error> {
        while let Some(((gen, _), _)) = self.explore_queue.peek() {
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

            for (id, gen_time) in parents {
                let state = self.states.get_mut(&id).ok_or(Error::MissingState)?;

                if !state.contains(WalkFlags::Explored) {
                    *state |= WalkFlags::Explored;
                    self.explore_queue.insert(gen_time, id);
                }
            }
        }

        Ok(())
    }

    fn expand_topo_walk(&mut self, id: &oid) -> Result<(), Error> {
        let parents = self.collect_parents(id)?;

        self.process_parents(id, &parents)?;

        for (pid, (parent_gen, parent_commit_time)) in parents {
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
                let parent_ids = self
                    .collect_all_parents(&pid)?
                    .into_iter()
                    .map(|e| e.0)
                    .collect();

                self.topo_queue.push(
                    parent_commit_time,
                    Info {
                        id: pid,
                        parent_ids,
                    },
                );
            }
        }

        Ok(())
    }

    fn process_parents(
        &mut self,
        id: &oid,
        parents: &[(ObjectId, GenAndCommitTime)],
    ) -> Result<(), Error> {
        let state = self.states.get_mut(id).ok_or(Error::MissingState)?;

        if state.contains(WalkFlags::Added) {
            return Ok(());
        }

        *state |= WalkFlags::Added;

        // If the current commit is uninteresting we pass that on to ALL parents,
        // otherwise we pass SymmetricLeft and AncestryPath + Seen
        let (pass, insert) = if state.contains(WalkFlags::Uninteresting) {
            let flags = WalkFlags::Uninteresting.into();

            for (id, _) in parents {
                let grand_parents = self.collect_all_parents(id)?;

                for (id, _) in &grand_parents {
                    self.states
                        .entry(*id)
                        .and_modify(|s| *s |= WalkFlags::Uninteresting)
                        .or_insert(WalkFlags::Uninteresting | WalkFlags::Seen);
                }
            }

            (flags, flags)
        } else {
            let flags = *state & (WalkFlags::SymmetricLeft | WalkFlags::AncestryPath);
            (flags, flags | WalkFlags::Seen)
        };

        for (id, _) in parents {
            self.states
                .entry(*id)
                .and_modify(|s| *s |= pass)
                .or_insert(insert);
        }

        Ok(())
    }

    fn collect_parents(
        &mut self,
        id: &oid,
    ) -> Result<SmallVec<[(ObjectId, GenAndCommitTime); 1]>, Error> {
        collect_parents(
            self.commit_graph.as_ref(),
            &self.find,
            id,
            matches!(self.parents, Parents::First),
            &mut self.buf,
        )
    }

    // Same as collect_parents but disregards the first_parent flag
    fn collect_all_parents(
        &mut self,
        id: &oid,
    ) -> Result<SmallVec<[(ObjectId, GenAndCommitTime); 1]>, Error> {
        collect_parents(
            self.commit_graph.as_ref(),
            &self.find,
            id,
            false,
            &mut self.buf,
        )
    }

    fn pop_commit(&mut self) -> Option<Result<Info, Error>> {
        let id = self.topo_queue.pop()?;

        let i = match self.indegrees.get_mut(&id.id) {
            Some(i) => i,
            None => {
                return Some(Err(Error::MissingIndegree));
            }
        };

        *i = 0;

        match self.expand_topo_walk(&id.id) {
            Ok(_) => (),
            Err(e) => {
                return Some(Err(e));
            }
        };

        Some(Ok(id))
    }
}

#[cfg_attr(feature = "trace", trace(prefix_enter = "", prefix_exit = ""))]
impl<Find, Predicate> Iterator for Walk<Find, Predicate>
where
    Find: gix_object::Find,
    Predicate: FnMut(&oid) -> bool,
{
    type Item = Result<Info, Error>;

    fn next(&mut self) -> Option<Self::Item> {
        match self.pop_commit()? {
            Ok(id) => {
                if (self.predicate)(&id.id) {
                    Some(Ok(id))
                } else {
                    self.next()
                }
            }
            Err(e) => Some(Err(e)),
        }
    }
}

#[cfg(feature = "standalone")]
enum Either<'buf, 'cache> {
    CommitRefIter(gix_object::CommitRefIter<'buf>),
    CachedCommit(gix_commitgraph::file::Commit<'cache>),
}
#[cfg(not(feature = "standalone"))]
use crate::Either;

#[cfg(feature = "standalone")]
fn find<'cache, 'buf, Find>(
    cache: Option<&'cache gix_commitgraph::Graph>,
    find: Find,
    id: &oid,
    buf: &'buf mut Vec<u8>,
) -> Result<Either<'buf, 'cache>, gix_object::find::existing_iter::Error>
where
    Find: gix_object::Find,
{
    match cache.and_then(|cache| cache.commit_by_id(id).map(Either::CachedCommit)) {
        Some(c) => Ok(c),
        None => find.find_commit_iter(id, buf).map(Either::CommitRefIter),
    }
}
#[cfg(not(feature = "standalone"))]
use crate::find;

fn collect_parents<'b, Find>(
    cache: Option<&gix_commitgraph::Graph>,
    f: Find,
    id: &oid,
    first_only: bool,
    buf: &'b mut Vec<u8>,
) -> Result<SmallVec<[(ObjectId, GenAndCommitTime); 1]>, Error>
where
    Find: gix_object::Find,
{
    let mut parents = SmallVec::<[(ObjectId, GenAndCommitTime); 1]>::new();

    match find(cache, &f, id, buf)? {
        Either::CommitRefIter(c) => {
            for token in c {
                use gix_object::commit::ref_iter::Token as T;
                match token {
                    Ok(T::Tree { .. }) => continue,
                    Ok(T::Parent { id }) => {
                        parents.push((id, (0, 0))); // Dummy numbers to be filled in
                        if first_only {
                            break;
                        }
                    }
                    Ok(_past_parents) => break,
                    Err(err) => return Err(err.into()),
                }
            }
            // Need to check the cache again. That a commit is not in the cache
            // doesn't mean a parent is not.
            for (id, gen_time) in parents.iter_mut() {
                let commit = find(cache, &f, id, buf)?;
                *gen_time = get_gen_and_commit_time(commit)?;
            }
        }
        Either::CachedCommit(c) => {
            for pos in c.iter_parents() {
                let parent_commit = cache
                    .expect("cache exists if CachedCommit was returned")
                    .commit_at(pos?);
                parents.push((
                    parent_commit.id().into(),
                    (
                        parent_commit.generation(),
                        parent_commit.committer_timestamp() as i64,
                    ),
                ));
                if first_only {
                    break;
                }
            }
        }
    };

    Ok(parents)
}

fn get_gen_and_commit_time(c: Either<'_, '_>) -> Result<GenAndCommitTime, Error> {
    match c {
        Either::CommitRefIter(c) => {
            let mut commit_time = 0;
            for token in c {
                use gix_object::commit::ref_iter::Token as T;
                match token {
                    Ok(T::Tree { .. }) => continue,
                    Ok(T::Parent { .. }) => continue,
                    Ok(T::Author { .. }) => continue,
                    Ok(T::Committer { signature }) => {
                        commit_time = signature.time.seconds;
                        break;
                    }
                    Ok(_unused_token) => break,
                    Err(err) => return Err(err.into()),
                }
            }
            Ok((gix_commitgraph::GENERATION_NUMBER_INFINITY, commit_time))
        }
        Either::CachedCommit(c) => Ok((c.generation(), c.committer_timestamp() as i64)),
    }
}

#[cfg(test)]
#[cfg(feature = "standalone")]
mod tests {
    use std::str::FromStr;
    use test_case::test_matrix;

    use super::*;
    use pretty_assertions::assert_eq;

    // Just to make the generated test case names a bit shorter
    use Parents::{All, First};
    use Sorting::{DateOrder, TopoOrder};

    enum GraphSetting {
        UseGraph,
        NoGraph,
    }
    use GraphSetting::{NoGraph, UseGraph};

    // To avoid not depending on the gix crate itself
    fn simple_parse(r: &str) -> gix_revision::Spec {
        if let Some((from, to)) = r.split_once("..") {
            gix_revision::Spec::Range {
                from: ObjectId::from_str(from).expect("Valid SHA1 in tests"),
                to: ObjectId::from_str(to).expect("Valid SHA1 in tests"),
            }
        } else if let Some(e) = r.strip_prefix("^") {
            gix_revision::Spec::Exclude(ObjectId::from_str(e).expect("Valid SHA1 in tests"))
        } else {
            gix_revision::Spec::Include(ObjectId::from_str(r).expect("Valid SHA1 in tests"))
        }
    }

    fn git_rev_list(
        graph_setting: GraphSetting,
        sorting: Sorting,
        parents: Parents,
        specs: &[&str],
    ) -> Vec<Info> {
        let git_flags = match graph_setting {
            UseGraph => &["-c", "core.commitGraph=true"],
            NoGraph => &["-c", "core.commitGraph=false"],
        };

        let rev_list_flags: &[&str] = match (parents, sorting) {
            (All, DateOrder) => &["--date-order"],
            (All, TopoOrder) => &["--topo-order"],
            (First, DateOrder) => &["--first-parent", "--date-order"],
            (First, TopoOrder) => &["--first-parent", "--topo-order"],
        };

        let output = std::process::Command::new("git")
            .args(git_flags)
            .args(&["rev-list", "--parents"])
            .args(rev_list_flags)
            .args(specs)
            .output()
            .expect("able to run git rev-list")
            .stdout;

        std::str::from_utf8(&output)
            .expect("sensible output from git rev-list")
            .split_terminator('\n')
            .map(|l| {
                let mut l = l.split_ascii_whitespace();
                Info {
                    id: ObjectId::from_str(l.next().expect("at least one object id"))
                        .expect("rev-list returns valid object ids"),
                    parent_ids: l
                        .map(ObjectId::from_str)
                        .collect::<Result<SmallVec<_>, _>>()
                        .expect("rev-list returns valid object ids"),
                }
            })
            .collect::<Vec<_>>()
    }

    fn test_body(
        graph_setting: GraphSetting,
        sorting: Sorting,
        parents: Parents,
        raw_specs: &[&str],
    ) {
        let store = gix_odb::at("../.git/objects").expect("find objects");
        let specs = raw_specs
            .iter()
            .map(|s| simple_parse(*s))
            .collect::<Vec<_>>();

        let commit_graph = match graph_setting {
            UseGraph => Some(
                gix_commitgraph::at(store.store_ref().path().join("info"))
                    .expect("commit graph available"),
                // The Walk takes an Option, but if the commit graph isn't
                // available I want to know immediately, hence the Some(...expect())
            ),
            NoGraph => None,
        };

        let walk = Builder::from_specs(&store, specs)
            .with_commit_graph(commit_graph)
            .sorting(sorting)
            .parents(parents)
            .build()
            .unwrap();

        let ids = walk.collect::<Result<Vec<_>, _>>().unwrap();
        let git_ids = git_rev_list(graph_setting, sorting, parents, raw_specs);

        assert_eq!(
            ids, git_ids,
            "left = ids, right = git_ids, flags = {parents:?} {sorting:?}"
        );
    }

    fn test_body_pred(
        graph_setting: GraphSetting,
        sorting: Sorting,
        parents: Parents,
        mut pred: impl FnMut(&oid) -> bool,
        raw_specs: &[&str],
    ) {
        let store = gix_odb::at("../.git/objects").expect("find objects");
        let specs = raw_specs
            .iter()
            .map(|s| simple_parse(*s))
            .collect::<Vec<_>>();

        let commit_graph = match graph_setting {
            UseGraph => Some(
                gix_commitgraph::at(store.store_ref().path().join("info"))
                    .expect("commit graph available"),
                // The Walk takes an Option, but if the commit graph isn't
                // available I want to know immediately, hence the Some(...expect())
            ),
            NoGraph => None,
        };

        let walk = Builder::from_specs(&store, specs)
            .with_commit_graph(commit_graph)
            .sorting(sorting)
            .parents(parents)
            .with_predicate(&mut pred)
            .build()
            .unwrap();

        let ids = walk.collect::<Result<Vec<_>, _>>().unwrap();
        let mut git_ids = git_rev_list(graph_setting, sorting, parents, raw_specs);
        git_ids.retain(|e| (&mut pred)(&e.id));

        assert_eq!(
            ids, git_ids,
            "left = ids, right = git_ids, flags = {parents:?} {sorting:?}"
        );
    }

    macro_rules! topo_test {
        ($test_name:ident, $($spec:literal),+) => {
            #[test_matrix(
                [ UseGraph, NoGraph ],
                [ DateOrder, TopoOrder ],
                [ All, First ]
            )]
            fn $test_name(graph_setting: GraphSetting, sorting: Sorting, parents: Parents) {
                test_body(graph_setting, sorting, parents, &[$($spec),+]);
            }
        };
    }

    macro_rules! topo_test_with_predicate {
        ($test_name:ident, $pred:expr, $($spec:literal),+) => {
            #[test_matrix(
                [ UseGraph, NoGraph ],
                [ DateOrder, TopoOrder ],
                [ All, First ]
            )]
            fn $test_name(graph_setting: GraphSetting, sorting: Sorting, parents: Parents) {
                test_body_pred(graph_setting, sorting, parents, $pred, &[$($spec),+]);
            }
        };
    }

    #[cfg(feature = "alltests")]
    include!("generated_tests.rs");

    topo_test!(basic, "b282e76b1322e1d26ef002968e1591bd8f22df96");
    topo_test!(
        one_end,
        "b282e76b1322e1d26ef002968e1591bd8f22df96",
        "^3be8265bc3f7d982170bd475be3b82cb140643b9"
    );

    topo_test!(
        empty_range,
        "3be8265bc3f7d982170bd475be3b82cb140643b9",
        "^b282e76b1322e1d26ef002968e1591bd8f22df96"
    );
    topo_test!(
        two_tips_two_ends,
        "d87231e63272c03850847902b86f0358e161210c",
        "00491e237a24c20f81e3e7f7a37d6359f65617d0",
        "^3be8265bc3f7d982170bd475be3b82cb140643b9",
        "^bb482759d46e81f0f51d7845d86d2dae93b8b3da"
    );

    topo_test_with_predicate!(
        basic_with_dummy_predicate,
        |oid| oid != ObjectId::from_str("bb8601cfa2f3bb33f9a8a9bdc4d66e3b598cddff").unwrap(),
        "b282e76b1322e1d26ef002968e1591bd8f22df96"
    );

    topo_test!(
        kekek,
        "bb8601cfa2f3bb33f9a8a9bdc4d66e3b598cddff",
        "^616867d9e3b817f505d4044ee3d81cfd348d579f"
    );
}
