#![allow(unused_must_use)]
#![allow(dead_code)]
#![allow(unused_imports)]

mod diffprinter;

use std::path::PathBuf;

use anyhow::Context;
use diffprinter::UnifiedDiffBuilder;
use gix::bstr::ByteSlice;
use gix::{bstr, config::tree::Diff};

use clap::{Args, Parser, Subcommand};

use gix::{diff, discover, hash, index, object, objs, Object, Repository};

use time::macros::format_description;

mod error;
use error::BlameDiffError;

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

    let prefix = repo.prefix().expect("have worktree")?;

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

    let diff = diff::blob::diff(
        diff::blob::Algorithm::Histogram,
        &input,
        UnifiedDiffBuilder::new(&input, old, new),
    );

    print!("{}", diff);

    Ok(())
}

fn cmd_blame(ba: BlameArgs) -> anyhow::Result<()> {
    let repo = gix::discover(".")?;
    let b = culpa::blame_file(&repo, &ba.revision, &ba.path, None)?;
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
