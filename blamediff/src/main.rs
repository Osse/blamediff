#![allow(unused_must_use)]
#![allow(dead_code)]
#![allow(unused_imports)]
#![allow(unused_variables)]

mod diffprinter;

use std::borrow::BorrowMut;
use std::cmp::Reverse;
// use std::borrow::{Borrow, BorrowMut};
use std::collections::{hash_map::Entry, HashMap, HashSet};
use std::default;
use std::path::PathBuf;

use anyhow::Context;
use diffprinter::UnifiedDiffBuilder;
use gix::bstr::ByteSlice;
use gix::prelude::FindExt;
use gix::{bstr, config::tree::Diff};

use clap::{Args, Parser, Subcommand};

use gix::{diff, discover, hash, index, object, objs, Object, ObjectId, Repository};

use time::macros::format_description;

mod error;
use error::BlameDiffError;

mod topo;

mod log;

#[derive(Args)]
struct DiffArgs {
    /// Old commit to diff
    #[arg(short, long)]
    old: Option<bstr::BString>,

    /// Old commit to diff
    #[arg(short, long)]
    new: Option<bstr::BString>,

    /// Paths to filter on
    paths: Vec<PathBuf>,
}

#[derive(Args)]
struct BlameArgs {
    revision: String,
    path: PathBuf,
}

#[derive(Args)]
struct TestArgs {
    args: Vec<String>,
}

#[derive(Args)]
struct LogArgs {
    #[arg(short, long)]
    first_parent: bool,

    revision: String,
}

#[derive(Parser)]
#[command(author, version, about, long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    Diff(DiffArgs),
    Blame(BlameArgs),
    Test(TestArgs),
    Log(LogArgs),
}

fn get_object(
    repo: &Repository,
    oid: impl Into<hash::ObjectId>,
    kind: object::Kind,
) -> anyhow::Result<Object> {
    repo.find_object(oid)?
        .peel_to_kind(kind)
        .map_err(|e| e.into())
}

fn resolve_tree<'a>(repo: &'a Repository, object: &bstr::BStr) -> anyhow::Result<gix::Tree<'a>> {
    let object = repo.rev_parse_single(object)?;
    get_object(repo, object, object::Kind::Tree).map(|o| o.into_tree())
}

fn main() -> anyhow::Result<()> {
    let args = Cli::parse();

    match args.command {
        Command::Diff(da) => cmd_diff(da),
        Command::Blame(ba) => cmd_blame(ba),
        Command::Test(ta) => cmd_test(ta),
        Command::Log(la) => cmd_log(la),
    }
}

pub struct BlobData<'a> {
    path: &'a bstr::BStr,
    id: gix::ObjectId,
}
pub const fn literal(x: &[u8]) -> &bstr::BStr {
    unsafe { core::mem::transmute(x) }
}
const DEV_NULL: BlobData = BlobData {
    path: literal(b"/dev/null"),
    id: gix::ObjectId::empty_blob(gix::index::hash::Kind::Sha1),
};

fn cmd_diff(da: DiffArgs) -> anyhow::Result<()> {
    let repo = discover(".")?;

    let prefix = repo
        .prefix()
        .expect("have worktree")
        .expect("have worktree");

    let owned_paths: Vec<bstr::BString> = da
        .paths
        .into_iter()
        .map(|p| prefix.join(p))
        .map(|p| bstr::BString::new(p.to_str().unwrap().as_bytes().to_owned()))
        .collect::<Vec<_>>();

    let paths = owned_paths.iter().map(|s| s.as_ref()).collect::<Vec<_>>();

    let old = da.old.unwrap_or(bstr::BString::from("HEAD"));
    let old = resolve_tree(&repo, old.as_ref())?;

    if let Some(arg) = da.new {
        let new = resolve_tree(&repo, arg.as_ref())?;

        diff_two_trees(old, new, &paths);
    } else {
        diff_with_disk(&repo, &paths);
    }

    Ok(())
}

fn disk_newer_than_index(
    stat: &index::entry::Stat,
    path: &std::path::Path,
) -> anyhow::Result<bool> {
    let fs_stat = std::fs::symlink_metadata(path)?;

    Ok((stat.mtime.secs as u64)
        < fs_stat
            .modified()?
            .duration_since(std::time::SystemTime::UNIX_EPOCH)?
            .as_secs())
}

fn diff_two_trees(
    tree_old: gix::Tree,
    tree_new: gix::Tree,
    paths: &[&bstr::BStr],
) -> anyhow::Result<()> {
    let mut platform = tree_old.changes()?;

    platform.track_path();

    let _outcome = platform.for_each_to_obtain_tree(&tree_new, |c| {
        use object::tree::diff::change::Event::*;
        let path = c.location;
        if paths.is_empty() || paths.iter().any(|&p| p == path) {
            match c.event {
                Addition {
                    entry_mode: objs::tree::EntryMode::Blob,
                    id,
                } => diff_blob_with_null(id, c.location, false),
                Deletion {
                    entry_mode: objs::tree::EntryMode::Blob,
                    id,
                } => diff_blob_with_null(id, c.location, true),
                Modification {
                    previous_entry_mode: objs::tree::EntryMode::Blob,
                    previous_id,
                    entry_mode: objs::tree::EntryMode::Blob,
                    id,
                } => diff_two_blobs(previous_id, id, c.location),
                x => {
                    dbg!(x);
                    Ok(())
                }
            }
            .map(|()| object::tree::diff::Action::Continue)
        } else {
            Ok(object::tree::diff::Action::Continue)
        }
    });

    Ok(())
}

fn diff_with_disk(repo: &Repository, paths: &[&bstr::BStr]) -> anyhow::Result<()> {
    let index = repo.open_index()?;
    for e in index.entries() {
        let p = e.path(&index);

        if paths.is_empty() || paths.iter().any(|&pp| pp == p) {
            let path = std::path::Path::new(p.to_str()?);

            if disk_newer_than_index(&e.stat, path)? {
                let disk_contents = std::fs::read_to_string(path)?;

                let blob = get_object(repo, e.id, object::Kind::Blob)?;
                let blob_contents = std::str::from_utf8(&blob.data)?;
                let input =
                    diff::blob::intern::InternedInput::new(blob_contents, disk_contents.as_str());

                let old = BlobData { id: e.id, path: p };
                let new = BlobData { id: e.id, path: p };

                let diff = diff::blob::diff(
                    diff::blob::Algorithm::Histogram,
                    &input,
                    UnifiedDiffBuilder::new(&input, old, new),
                );

                print!("{}", diff);
            }
        }
    }

    Ok(())
}

fn diff_blob_with_null(
    id: gix::Id,
    path: &bstr::BStr,
    to_null: bool,
) -> Result<(), BlameDiffError> {
    let data = &id.object()?.data;
    let file = std::str::from_utf8(data)?;

    let id = BlobData {
        id: id.detach(),
        path,
    };

    let input = if to_null {
        diff::blob::intern::InternedInput::new(file, "")
    } else {
        diff::blob::intern::InternedInput::new("", file)
    };

    let diff = diff::blob::diff(
        diff::blob::Algorithm::Histogram,
        &input,
        if to_null {
            UnifiedDiffBuilder::new(&input, DEV_NULL, id)
        } else {
            UnifiedDiffBuilder::new(&input, id, DEV_NULL)
        },
    );

    print!("{}", diff);

    Ok(())
}

fn diff_two_blobs(
    old_id: gix::Id,
    new_id: gix::Id,
    path: &bstr::BStr,
) -> Result<(), BlameDiffError> {
    let old_data = &old_id.object()?.data;
    let new_data = &new_id.object()?.data;

    let old_file = std::str::from_utf8(old_data)?;
    let new_file = std::str::from_utf8(new_data)?;

    let input = diff::blob::intern::InternedInput::new(old_file, new_file);

    let new = BlobData {
        id: new_id.detach(),
        path,
    };
    let old = BlobData {
        id: old_id.detach(),
        path,
    };

    let t = culpa::line_tracking::LineTracker::from_range(0..new_file.lines().count() as u32);

    let diff = diff::blob::diff(
        diff::blob::Algorithm::Histogram,
        &input,
        culpa::sinks::RangeAndLineCollector::new(&input, t),
    );

    print!("{:#?}", diff);

    Ok(())
}

fn cmd_blame(ba: BlameArgs) -> anyhow::Result<()> {
    let repo = gix::discover(".")?;
    let b = culpa::blame_file(&repo, &ba.revision, false, &ba.path)?;
    let format = format_description!(
        "[year]-[month]-[day] [hour]:[minute]:[second] [offset_hour sign:mandatory][offset_minute]"
    );

    for bl in b.blamed_lines() {
        let c = repo.find_object(bl.id)?.into_commit();

        let author = c.author().context("getting commit author")?;
        let name = author.name;
        let timestamp = author.time.format(format);
        let (boundary, short_hash) = if bl.boundary {
            ("^", c.id.to_hex_with_len(7))
        } else {
            ("", c.id.to_hex_with_len(8))
        };

        println!(
            "{boundary}{short_hash} ({name} {timestamp} {:2}) {}",
            bl.line_no + 1,
            bl.line,
        );
    }

    Ok(())
}

use gix::traverse::commit::*;

struct MyState {
    inner: ancestors::State,
    more_stuff: Vec<String>,
}

// impl Borrow<ancestors::State> for &MyState {
//     fn borrow(&self) -> &ancestors::State {
//         &self.inner
//     }
// }

// impl BorrowMut<ancestors::State> for &MyState {
//     fn borrow_mut(&mut self) -> &mut ancestors::State {
//         &mut self.inner
//     }
// }

// Info about a future merge commit
#[derive(Debug)]
struct DescendedMerge {
    chain: usize,
    info: Info,
    total: usize,
}

fn cmd_test(ta: TestArgs) -> anyhow::Result<()> {
    Ok(())
}

// fn cmd_test(ta: TestArgs) -> anyhow::Result<()> {
//     let repo = &gix::discover(".")?;
//     let head = repo.rev_parse_single(ta.args[0].as_str())?;

//     let state = ancestors::State::default();

//     let mut merges = HashMap::<gix::ObjectId, DescendedMerge>::new();
//     let mut seen = HashSet::<gix::ObjectId>::new();

//     let ancestors = Ancestors::new(
//         std::iter::once(head),
//         state,
//         |oid: &gix::oid, buf: &mut Vec<u8>| repo.objects.find_commit_iter(oid, buf),
//     )
//     .sorting(Sorting::ByCommitTimeNewestFirst)?;

//     for (c, chain) in ancestors.map(|c| {
//         let commit = c.unwrap();
//         seen.insert(commit.id);

//         let total = commit.parent_ids.len();

//         if total > 1 {
//             // This is a merge commit. Start tracking it.
//             for (i, parent) in commit.parent_ids.iter().enumerate() {
//                 merges.insert(
//                     *parent,
//                     DescendedMerge {
//                         chain: i,
//                         info: commit.clone(),
//                         total,
//                     },
//                 );
//             }
//         }

//         let chain = match merges.entry(commit.id) {
//             Entry::Occupied(e) => {
//                 // This commit is an ancestor of a merge we know about
//                 let (_, descended_merge) = e.remove_entry();

//                 let mut chain = Some(descended_merge.chain);

//                 if commit.parent_ids.len() > 0 {
//                     let pid = *commit.parent_ids.first().unwrap();

//                     if pid == descended_merge.info.id {
//                         chain = None;
//                     }

//                     if merges.contains_key(&pid) {
//                         chain = None;
//                     } else {
//                         // Update to indicate that this commit's parent is now
//                         // know to be an ancestor of a merge we know about
//                         merges.insert(
//                             pid,
//                             DescendedMerge {
//                                 chain: descended_merge.chain,
//                                 info: descended_merge.info,
//                                 total: descended_merge.total,
//                             },
//                         );
//                     }
//                 }

//                 chain
//             }
//             Entry::Vacant(_) => {
//                 // This commit is not an ancestor of a merge commit we know about so there is no chain
//                 None
//             }
//         };

//         // dbg!(&commit, &merges);

//         (commit, chain)
//     }) {
//         if let Some(chain) = chain {
//             if chain == 0 {
//                 println!("* | {}", c.id);
//             } else {
//                 println!("| * {}", c.id);
//             }
//         } else {
//             println!("* {}", c.id);
//         }
//         if c.parent_ids.len() > 1 {
//             println!("|\\");
//         }
//     }

//     Ok(())
// }

fn cmd_log(la: LogArgs) -> anyhow::Result<()> {
    let repo = discover(".")?;
    let revision = la.revision.as_str();

    let range = repo.rev_parse(revision)?.detach();

    use gix::revision::plumbing::Spec;
    let (start, end) = match range {
        Spec::Include(oid) => (repo.find_object(oid)?.id, None),
        Spec::Exclude(oid) => (repo.rev_parse_single("HEAD")?.object()?.id, Some(oid)),
        Spec::Range { from, to } => (to, Some(from)),
        _ => return Err(anyhow::anyhow!("Invalid range")),
    };

    let rev_walker = {
        let r = repo
            .rev_walk(std::iter::once(start))
            .sorting(gix::traverse::commit::Sorting::BreadthFirst);

        if la.first_parent {
            r.first_parent_only()
        } else {
            r
        }
    }
    .all()?;

    let commit_graph = repo.commit_graph()?;

    let history: Result<Vec<_>, _> = rev_walker.into_iter().collect();
    let history = history?;

    // TODO: rev_walker.map_while() directly instead of collecting everything into a Vec

    // Collect commits that don't have any generation numbers. As soon as a gen
    // number is found we know all subsequent (ie. earlier in history) have.
    let missing_gens: Vec<_> = history
        .iter()
        .map_while(|h| {
            if commit_graph.lookup(h.id).is_none() {
                Some(h)
            } else {
                None
            }
        })
        .collect();

    let mut my_gen_numbers = HashMap::<gix::ObjectId, u32>::new();

    // Walk in reverse
    for h in missing_gens.into_iter().rev() {
        // A commit's generation number is m + 1 where m is the maximum generation number of its parents
        let gen = h
            .parent_ids()
            .map(|pid| {
                // Get gen number either from the commit graph or a value previously calculated in my_gen_numbers
                commit_graph.commit_by_id(&pid).map_or_else(
                    || {
                        *my_gen_numbers
                            .get(&pid.detach())
                            .expect("have gen number inserted")
                    },
                    |o| o.generation(),
                )
            })
            .max()
            .unwrap_or(0)
            + 1;

        if my_gen_numbers.insert(h.id, gen).is_some() {
            panic!("was already there");
        }
    }

    // Copy rest of history numbers to avoid having to lookup two places:
    for c in commit_graph.iter_commits() {
        my_gen_numbers.insert(ObjectId::from(c.id()), c.generation());
    }

    let topo_walker = topo::TopoWalker::new(
        &repo,
        std::iter::once(start),
        std::iter::empty::<gix::ObjectId>(),
    )?;

    for c in topo_walker {
        println!("{}", c?.to_hex());
    }

    Ok(())
}
