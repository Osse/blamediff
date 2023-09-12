use std::cell::{RefCell, RefMut};
use std::collections::BinaryHeap;
use std::collections::{hash_map::Entry, HashMap};
use std::rc::Rc;

use flagset::{flags, FlagSet};

flags! {
    enum WalkFlags: u32 {
        Explored,
        InDegree,
        Uninteresting,
        Added,
        SymmetricLeft,
        AncestryPath
    }
}

#[derive(Debug, Eq)]
struct Dank {
    id: gix::ObjectId,
    gen: u32,
    flags: RefCell<FlagSet<WalkFlags>>,
}

impl std::cmp::Ord for Dank {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.gen.cmp(&other.gen)
    }
}
impl std::cmp::PartialOrd for Dank {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        self.gen.partial_cmp(&other.gen)
    }
}

impl std::cmp::PartialEq for Dank {
    fn eq(&self, other: &Self) -> bool {
        self.gen == other.gen
    }
}

pub struct TopoWalker<'a> {
    repo: &'a gix::Repository,
    commit_graph: gix::commitgraph::Graph,
    indegrees: HashMap<gix::ObjectId, i32>,
    danks: HashMap<gix::ObjectId, Rc<Dank>>,
    explore_queue: std::collections::BinaryHeap<Rc<Dank>>,
    indegree_queue: std::collections::BinaryHeap<Rc<Dank>>,
    topo_queue: std::vec::Vec<Rc<Dank>>,
    min_gen: u32,
}

impl<'a> TopoWalker<'a> {
    /// Create a new TopoWalker that walks the given repository
    pub fn on_repo(repo: &'a gix::Repository) -> Result<Self, gix::commitgraph::init::Error> {
        Ok(Self {
            repo,
            commit_graph: repo.commit_graph()?,
            indegrees: HashMap::new(),
            danks: HashMap::new(),
            explore_queue: std::collections::BinaryHeap::new(),
            indegree_queue: std::collections::BinaryHeap::new(),
            topo_queue: vec![],
            min_gen: u32::MAX,
        })
    }

    pub fn add_tip(&mut self, id: gix::ObjectId) {
        *self.indegrees.entry(id).or_default() = 1;

        let gen = self.commit_graph.commit_by_id(id).unwrap().generation();

        self.min_gen = gen;

        let start_dank = Rc::new(Dank {
            id,
            gen,
            flags: RefCell::new(WalkFlags::Explored | WalkFlags::InDegree),
        });

        self.danks.insert(id, start_dank.clone());

        self.explore_queue.push(start_dank.clone());
        self.indegree_queue.push(start_dank.clone());

        self.compute_indegree_to_depth(gen);

        dbg!("adding to topo_queue", start_dank.clone().id);
        self.topo_queue.push(start_dank.clone());

        self.topo_queue.reverse();
    }

    fn compute_indegree_to_depth(&mut self, gen_cutoff: u32) {
        if let Some(c) = self.indegree_queue.peek() {
            dbg!("compute_indegree_to_depth", c.id, c.gen, gen_cutoff);
        }
        while let Some(c) = self.indegree_queue.peek() {
            if c.gen >= gen_cutoff {
                self.indegree_walk_step();
            } else {
                break;
            }
        }
    }

    fn indegree_walk_step(&mut self) {
        if let Some(c) = self.indegree_queue.pop() {
            // dbg!("indegree_walk_step", &c);
            self.explore_to_depth(c.gen);

            let commit = self.commit_graph.commit_by_id(c.id).expect("find");
            for p in commit.iter_parents() {
                let parent_commit = self.commit_graph.commit_at(p.expect("get position"));
                let pid = gix::ObjectId::from(parent_commit.id());
                let i = self.indegrees.entry(pid).or_default();

                // dbg!(&pid, *i);

                if *i != 0 {
                    *i += 1;
                } else {
                    *i = 2;
                }

                let dank = &self.danks[&pid];
                // dbg!(&dank);

                if !dank.flags.borrow().contains(WalkFlags::InDegree) {
                    *dank.flags.borrow_mut() |= WalkFlags::InDegree;
                    self.indegree_queue.push(dank.clone());
                }
            }
        }
    }

    fn explore_to_depth(&mut self, gen_cutoff: u32) {
        while let Some(c) = self.explore_queue.peek() {
            if c.gen >= gen_cutoff {
                self.explore_walk_step();
            } else {
                break;
            }
        }
    }

    fn explore_walk_step(&mut self) {
        if let Some(c) = self.explore_queue.pop() {
            self.process_parents(c.clone());

            let commit = self.commit_graph.commit_by_id(c.id).expect("find");
            for p in commit.iter_parents() {
                let parent_commit = self.commit_graph.commit_at(p.expect("get position"));
                let dank = self.danks.get(parent_commit.id()).unwrap();

                if !dank.flags.borrow().contains(WalkFlags::Explored) {
                    *dank.flags.borrow_mut() |= WalkFlags::Explored;
                    self.explore_queue.push(dank.clone());
                }
            }
        }
    }

    fn expand_topo_walk(&mut self, d: Rc<Dank>) {
        let parents = self
            .commit_graph
            .commit_by_id(d.id)
            .expect("commit_by_id")
            .iter_parents()
            .collect::<Result<Vec<_>, _>>()
            .expect("collect parents");

        self.process_parents(d.clone());

        for p in parents {
            let parent_gen = self.commit_graph.commit_at(p).generation();
            let pid = gix::ObjectId::from(self.commit_graph.commit_at(p).id());

            if parent_gen < self.min_gen {
                self.min_gen = parent_gen;
                self.compute_indegree_to_depth(self.min_gen);
            }

            let i = self.indegrees.get_mut(&pid).unwrap();
            *i -= 1;

            if *i == 1 {
                let pd = self.danks.get(&pid).expect("foo");
                dbg!("adding to topo_queue", pd.clone().id);
                self.topo_queue.push(pd.clone());
            }
        }
    }

    fn process_parents(&mut self, c: Rc<Dank>) -> anyhow::Result<()> {
        if c.flags.borrow().contains(WalkFlags::Added) {
            return Ok(());
        }
        *c.flags.borrow_mut() |= WalkFlags::Added;

        let pass_flags = *c.flags.borrow() & (WalkFlags::SymmetricLeft | WalkFlags::AncestryPath);

        if !c.flags.borrow().contains(WalkFlags::Uninteresting) {
            let commit = self.commit_graph.commit_by_id(c.id).expect("find");
            for p in commit.iter_parents() {
                let parent_commit = self.commit_graph.commit_at(p.expect("get position"));

                let pid = gix::ObjectId::from(parent_commit.id());

                match self.danks.entry(pid) {
                    Entry::Occupied(o) => {
                        *o.get().flags.borrow_mut() |= pass_flags;
                    }
                    Entry::Vacant(v) => {
                        v.insert(Rc::new(Dank {
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
        let c = self.topo_queue.pop()?;

        let i = self.indegrees.get_mut(&c.id).unwrap();
        *i = 0;

        self.expand_topo_walk(c.clone());

        Some(c.id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run_git_rev_list(tag: &str) -> Vec<String> {
        let output = std::process::Command::new("git")
            .args(["rev-list", "--topo-order", tag])
            .output()
            .expect("able to run git rev-list")
            .stdout;

        let output = std::str::from_utf8(&output).expect("valid UTF-8");

        output.split_terminator('\n').map(String::from).collect()
    }

    #[test]
    fn first_test() {
        let r = gix::discover(".").unwrap();
        let mut t = TopoWalker::on_repo(&r).unwrap();
        let tip = r.rev_parse_single("first-test").unwrap();
        t.add_tip(tip.detach());

        let mine = t.map(|id| id.to_hex().to_string()).collect::<Vec<_>>();
        let fasit = run_git_rev_list("first-test");
        assert_eq!(mine, fasit);
    }

    #[test]
    fn second_test() {
        let r = gix::discover(".").unwrap();
        let mut t = TopoWalker::on_repo(&r).unwrap();
        let tip = r.rev_parse_single("second-test").unwrap();
        t.add_tip(tip.detach());

        let mine = t.map(|id| id.to_hex().to_string()).collect::<Vec<_>>();
        let fasit = run_git_rev_list("second-test");
        assert_eq!(mine, fasit);
    }
}
