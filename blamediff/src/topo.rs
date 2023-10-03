use gix::hashtable::{hash_map::Entry, HashMap};
use std::borrow::BorrowMut;
use std::cell::OnceCell;
use std::cell::{RefCell, RefMut};
use std::collections::BinaryHeap;
use std::rc::Rc;

use flagset::{flags, FlagSet};

thread_local! {
static INDENT: RefCell<usize> = RefCell::new(0);
}

struct Increaser;

impl Increaser {
    fn new() -> Self {
        INDENT.with(|i| {
            *i.borrow_mut() += 4;
        });

        Self {}
    }
}

impl Drop for Increaser {
    fn drop(&mut self) {
        INDENT.with(|i| {
            *i.borrow_mut() -= 4;
        });
    }
}

macro_rules! function {
    () => {{
        fn f() {}
        fn type_name_of<T>(_: T) -> &'static str {
            std::any::type_name::<T>()
        }
        let name = type_name_of(f);

        // Find and cut the rest of the path
        match &name[..name.len() - 3].rfind(':') {
            Some(pos) => &name[pos + 1..name.len() - 3],
            None => &name[..name.len() - 3],
        }
    }};
}

macro_rules! topodbg {
    () => {
        ::std::eprintln!("{}[{}]", " ".repeat(INDENT.with(|i| *i.borrow())),
            function!());
    };
    ($val:expr $(,)?) => {
        match $val {
            tmp => {
                ::std::eprintln!("{}[{}] {} = {:?}", " ".repeat(INDENT.with(|i| *i.borrow())),
                    function!(), ::std::stringify!($val), &tmp);
                tmp
            }
        }
    };
    ($($val:expr),+ $(,)?) => {
        ($($crate::topodbg!($val)),+,)
    };
}

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

#[derive(Debug, Eq)]
struct Item {
    id: gix::ObjectId,
    gen: u32,
    flags: RefCell<FlagSet<WalkFlags>>,
}

impl std::cmp::Ord for Item {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.gen.cmp(&other.gen)
    }
}
impl std::cmp::PartialOrd for Item {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        self.gen.partial_cmp(&other.gen)
    }
}

impl std::cmp::PartialEq for Item {
    fn eq(&self, other: &Self) -> bool {
        self.gen == other.gen
    }
}

#[derive(Debug)]
pub enum Error {
    MissingIndegree,
    MissingItem,
    CommitNotFound,
    CommitGraphInit(gix::commitgraph::init::Error),
    CommitGraphFile(gix::commitgraph::file::commit::Error),
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::MissingIndegree => write!(f, "Calculated indegree missing"),
            Error::MissingItem => write!(f, "Internal item not added"),
            Error::CommitNotFound => write!(f, "Commit not found in commit graph"),
            Error::CommitGraphInit(e) => write!(f, "Error initializing graph: {}", e),
            Error::CommitGraphFile(e) => write!(f, "Error doing file stuff: {}", e),
        }
    }
}

impl std::error::Error for Error {}

macro_rules! make_error {
    ($e:ty, $b:ident) => {
        impl From<$e> for Error {
            fn from(e: $e) -> Self {
                Error::$b(e)
            }
        }
    };
}
make_error![gix::commitgraph::init::Error, CommitGraphInit];
make_error![gix::commitgraph::file::commit::Error, CommitGraphFile];

/// a walker that walks in topographical order, like `git rev-list --topo-order`.
pub struct TopoWalker<'a> {
    repo: &'a gix::Repository,
    commit_graph: gix::commitgraph::Graph,
    indegrees: HashMap<gix::ObjectId, i32>,
    items: HashMap<gix::ObjectId, Rc<Item>>,
    explore_queue: BinaryHeap<Rc<Item>>,
    indegree_queue: BinaryHeap<Rc<Item>>,
    topo_queue: Vec<Rc<Item>>,
    min_gen: u32,
}

impl<'a> TopoWalker<'a> {
    /// Create a new TopoWalker that walks the given repository
    pub fn on_repo(
        repo: &'a gix::Repository,
        tips: impl IntoIterator<Item = impl Into<gix::ObjectId>>,
        bottoms: impl IntoIterator<Item = impl Into<gix::ObjectId>>,
    ) -> Result<Self, Error> {
        let mut indegrees = HashMap::default();
        let mut items = HashMap::default();
        let commit_graph = repo.commit_graph()?;

        let mut explore_queue = BinaryHeap::new();
        let mut indegree_queue = BinaryHeap::new();
        let mut min_gen = u32::MAX;

        let tips = tips.into_iter().map(Into::into).collect::<Vec<_>>();
        let tip_flags = WalkFlags::Explored | WalkFlags::InDegree;
        let bottom_flags = tip_flags | WalkFlags::Uninteresting | WalkFlags::Bottom;
        let bottoms = bottoms.into_iter().map(Into::into).collect::<Vec<_>>();

        for (id, flags) in tips
            .iter()
            .map(|id| (id, tip_flags))
            .chain(bottoms.iter().map(|id| (id, bottom_flags)))
        {
            *indegrees.entry(*id).or_default() = 1;

            let gen = commit_graph
                .commit_by_id(id)
                .ok_or(Error::CommitNotFound)?
                .generation();

            if gen < min_gen {
                min_gen = gen;
            }

            let item = Rc::new(Item {
                id: *id,
                gen,
                flags: RefCell::new(flags),
            });

            if !tip_flags.contains(WalkFlags::Uninteresting) {
                items.insert(*id, item.clone());
                explore_queue.push(item.clone());
                indegree_queue.push(item.clone());
            }
        }

        let mut s = Self {
            repo,
            commit_graph,
            indegrees,
            items,
            explore_queue,
            indegree_queue,
            topo_queue: vec![],
            min_gen,
        };

        s.compute_indegree_to_depth(min_gen)?;

        for id in tips.iter().chain(bottoms.iter()) {
            let i = *s.indegrees.get(id).ok_or(Error::MissingIndegree)?;

            if i == 1 {
                dbg!(id);
                s.topo_queue.push(s.items[id].clone());
            }
        }

        s.topo_queue.reverse();

        Ok(s)
    }

    fn compute_indegree_to_depth(&mut self, gen_cutoff: u32) -> Result<(), Error> {
        let i = Increaser::new();
        topodbg!(gen_cutoff);
        while let Some(c) = self.indegree_queue.peek() {
            if c.gen >= gen_cutoff {
                self.indegree_walk_step()?;
            } else {
                break;
            }
        }

        Ok(())
    }

    fn indegree_walk_step(&mut self) -> Result<(), Error> {
        let i = Increaser::new();
        topodbg!();
        if let Some(c) = self.indegree_queue.pop() {
            self.explore_to_depth(c.gen);

            let commit = self
                .commit_graph
                .commit_by_id(c.id)
                .ok_or(Error::CommitNotFound)?;

            for p in commit.iter_parents() {
                let parent_commit = self.commit_graph.commit_at(p.expect("get position"));
                let pid = gix::ObjectId::from(parent_commit.id());

                self.indegrees
                    .entry(pid)
                    .and_modify(|e| *e += 1)
                    .or_insert(2);

                let item = &self.items[&pid];

                if !item.flags.borrow().contains(WalkFlags::InDegree) {
                    *item.flags.borrow_mut() |= WalkFlags::InDegree;
                    self.indegree_queue.push(item.clone());
                }
            }
        }

        Ok(())
    }

    fn explore_to_depth(&mut self, gen_cutoff: u32) -> Result<(), Error> {
        let i = Increaser::new();
        topodbg!();
        while let Some(c) = self.explore_queue.peek() {
            if c.gen >= gen_cutoff {
                self.explore_walk_step()?;
            } else {
                break;
            }
        }

        Ok(())
    }

    fn explore_walk_step(&mut self) -> Result<(), Error> {
        let i = Increaser::new();
        topodbg!();
        if let Some(c) = self.explore_queue.pop() {
            self.process_parents(c.clone());

            let commit = self.commit_graph.commit_by_id(c.id).expect("find");
            for p in commit.iter_parents() {
                let parent_commit = self.commit_graph.commit_at(p.expect("get position"));
                let item = self
                    .items
                    .get(parent_commit.id())
                    .ok_or(Error::MissingItem)?;

                if !item.flags.borrow().contains(WalkFlags::Explored) {
                    *item.flags.borrow_mut() |= WalkFlags::Explored;
                    self.explore_queue.push(item.clone());
                }
            }
        }

        Ok(())
    }

    fn expand_topo_walk(&mut self, d: Rc<Item>) -> Result<(), Error> {
        let i = Increaser::new();
        topodbg!(&d);
        let parents = self
            .commit_graph
            .commit_by_id(d.id)
            .ok_or(Error::CommitNotFound)?
            .iter_parents()
            .collect::<Result<Vec<_>, _>>()?;

        self.process_parents(d.clone());

        for p in parents {
            let parent_gen = self.commit_graph.commit_at(p).generation();
            let pid = gix::ObjectId::from(self.commit_graph.commit_at(p).id());

            let pd = self.items.get(&pid).expect("item already added").clone();

            if pd.flags.borrow().contains(WalkFlags::Uninteresting) {
                continue;
            }

            if parent_gen < self.min_gen {
                self.min_gen = parent_gen;
                self.compute_indegree_to_depth(self.min_gen);
            }

            let i = self.indegrees.get_mut(&pid).ok_or(Error::MissingIndegree)?;

            *i -= 1;

            if *i == 1 {
                topodbg!(&pid);
                self.topo_queue.push(pd.clone());
            }
        }

        Ok(())
    }

    fn process_parents(&mut self, c: Rc<Item>) -> Result<(), Error> {
        let i = Increaser::new();
        topodbg!(&c);
        if c.flags.borrow().contains(WalkFlags::Added) {
            return Ok(());
        }

        *c.flags.borrow_mut() |= WalkFlags::Added;

        if !c.flags.borrow().contains(WalkFlags::Uninteresting) {
            let pass_flags =
                *c.flags.borrow() & (WalkFlags::SymmetricLeft | WalkFlags::AncestryPath);
            let commit = self
                .commit_graph
                .commit_by_id(c.id)
                .ok_or(Error::CommitNotFound)?;
            for p in commit.iter_parents() {
                let parent_commit = self.commit_graph.commit_at(p.expect("get position"));

                let pid = gix::ObjectId::from(parent_commit.id());

                match self.items.entry(pid) {
                    Entry::Occupied(o) => {
                        *o.get().flags.borrow_mut() |= pass_flags;
                    }
                    Entry::Vacant(v) => {
                        v.insert(Rc::new(Item {
                            id: pid,
                            gen: parent_commit.generation(),
                            flags: RefCell::new(pass_flags),
                        }));
                    }
                };
            }
        } else {
            let pass_flags = *c.flags.borrow() & (WalkFlags::Uninteresting);
            let commit = self
                .commit_graph
                .commit_by_id(c.id)
                .ok_or(Error::CommitNotFound)?;
            for p in commit.iter_parents() {
                let parent_commit = self.commit_graph.commit_at(p.expect("get position"));

                let pid = gix::ObjectId::from(parent_commit.id());

                match self.items.entry(pid) {
                    Entry::Occupied(o) => {
                        *o.get().flags.borrow_mut() |= pass_flags;
                    }
                    Entry::Vacant(v) => {
                        v.insert(Rc::new(Item {
                            id: pid,
                            gen: parent_commit.generation(),
                            flags: RefCell::new(pass_flags),
                        }));
                    }
                };
            }
        }

        Ok(())
    }
}

impl<'a> Iterator for TopoWalker<'a> {
    type Item = gix::ObjectId;

    fn next(&mut self) -> Option<Self::Item> {
        let i = Increaser::new();
        topodbg!();
        let c = self.topo_queue.pop()?;

        let i = self
            .indegrees
            .get_mut(&c.id)
            .expect("indegree already calculated");
        *i = 0;

        self.expand_topo_walk(c.clone());

        Some(c.id)
    }
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

        let mine = t.map(|id| id.to_hex().to_string()).collect::<Vec<_>>();
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

        let mine = t.map(|id| id.to_hex().to_string()).collect::<Vec<_>>();
        let fasit = run_git_rev_list(&["second-test"]);
        assert_eq!(mine, fasit);
    }

    #[test]
    fn first_limited_test() {
        let r = gix::discover(".").unwrap();
        let tip = r.rev_parse_single("first-test").unwrap();
        let bottom = r.rev_parse_single("6a30c80").unwrap();
        let t = TopoWalker::on_repo(&r, std::iter::once(tip), std::iter::once(bottom)).unwrap();

        let mine = t.map(|id| id.to_hex().to_string()).collect::<Vec<_>>();
        let fasit = run_git_rev_list(&["6a30c80..first-test"]);
        assert_eq!(mine, fasit);
    }

    #[test]
    fn second_limited_test() {
        let r = gix::discover(".").unwrap();
        let tip = r.rev_parse_single("second-test").unwrap();
        let bottom = r.rev_parse_single("6a30c80").unwrap();
        let t = TopoWalker::on_repo(&r, std::iter::once(tip), std::iter::once(bottom)).unwrap();

        let mine = t.map(|id| id.to_hex().to_string()).collect::<Vec<_>>();
        let fasit = run_git_rev_list(&["6a30c80..second-test"]);
        assert_eq!(mine, fasit);
    }

    #[test]
    fn second_limited_test_left() {
        let r = gix::discover(".").unwrap();
        let tip = r.rev_parse_single("second-test").unwrap();
        let bottom = r.rev_parse_single("8bf8780").unwrap();
        let t = TopoWalker::on_repo(&r, std::iter::once(tip), std::iter::once(bottom)).unwrap();

        let mine = t.map(|id| id.to_hex().to_string()).collect::<Vec<_>>();
        let fasit = run_git_rev_list(&["8bf8780..second-test"]);
        assert_eq!(mine, fasit);
    }

    #[test]
    fn second_limited_test_right() {
        let r = gix::discover(".").unwrap();
        let tip = r.rev_parse_single("second-test").unwrap();
        let bottom = r.rev_parse_single("bb48275").unwrap();
        let t = TopoWalker::on_repo(&r, std::iter::once(tip), std::iter::once(bottom)).unwrap();

        let mine = t.map(|id| id.to_hex().to_string()).collect::<Vec<_>>();
        let fasit = run_git_rev_list(&["bb48275..second-test"]);
        assert_eq!(mine, fasit);
    }
}
