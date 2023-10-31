static CELL: std::sync::OnceLock<Sorting> = std::sync::OnceLock::new();

#[derive(Debug, Eq, PartialEq, PartialOrd, Ord)]
struct Key {
    commit_time: i64,
}

impl Key {
    fn new(commit_time: i64) -> Self {
        Key { commit_time }
    }
}

// impl std::cmp::Ord for Key {
//     fn cmp(&self, other: &Self) -> std::cmp::Ordering {
//         match CELL.get() {
//             Some(Sorting::DateOrder) => self
//                 .commit_time
//                 .cmp(&other.commit_time)
//                 .then(self.generation.cmp(&other.generation)),
//             _ => self
//                 .generation
//                 .cmp(&other.generation)
//                 .then(self.commit_time.cmp(&other.commit_time)),
//         }
//     }
// }

// impl std::cmp::PartialOrd for Key {
//     fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
//         Some(self.commit_time.cmp(&other.commit_time))
//     }
// }

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

// macro_rules! topo_test {
//     ($test_name:ident, $range:literal) => {
//         #[test]
//         fn $test_name() {
//             let repo = gix::discover(".").unwrap();
//             let (start, end) = resolve(&repo, $range);

//             for (flag, sorting) in [
//                 ("--date-order", Sorting::TopoOrder),
//                 ("--topo-order", Sorting::TopoOrder),
//             ] {
//                 let walk = Walk2::new(
//                     |id, buf| repo.objects.find_commit_iter(id, buf),
//                     std::iter::once(start),
//                     end.map(|e| std::iter::once(e)),
//                 )
//                 .unwrap();

//                 compare(walk, flag, $range);
//             }
//         }
//     };
// }
