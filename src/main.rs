#![allow(unused_must_use)]
#![allow(dead_code)]

mod diffprinter;

use std::path::PathBuf;

use diffprinter::UnifiedDiffBuilder;
use gix::bstr;
use gix::bstr::ByteSlice;

use clap::Parser;

use gix::{diff, discover, hash, index, object, objs, Object, Repository};

mod error;
use error::BlameDiffError;

#[derive(Parser)]
#[command(author, version, about, long_about = None)]
struct Args {
    /// Old commit to diff
    #[arg(short, long)]
    old: Option<bstr::BString>,

    /// Old commit to diff
    #[arg(short, long)]
    new: Option<bstr::BString>,

    /// Paths to filter on
    paths: Vec<PathBuf>,
}

fn get_object<'a>(
    repo: &'a Repository,
    oid: impl Into<hash::ObjectId>,
    kind: object::Kind,
) -> Result<Object<'a>, BlameDiffError> {
    repo.try_find_object(oid)?
        .ok_or(BlameDiffError::BadArgs)?
        .peel_to_kind(kind)
        .map_err(|_| BlameDiffError::BadArgs)
}

fn resolve_tree<'a>(
    repo: &'a Repository,
    object: &bstr::BStr,
) -> Result<Object<'a>, BlameDiffError> {
    let object = repo
        .rev_parse(object)?
        .single()
        .ok_or(BlameDiffError::BadArgs)?;

    get_object(repo, object, object::Kind::Tree)
}

fn main() -> Result<(), BlameDiffError> {
    let args = Args::parse();

    let repo = discover(".")?;

    let prefix = repo.prefix().expect("have worktree")?;

    let paths = args
        .paths
        .into_iter()
        .map(|p| prefix.join(p))
        .collect::<Vec<_>>();

    let old = args.old.unwrap_or(bstr::BString::from("HEAD"));
    let old = resolve_tree(&repo, old.as_ref())?;
    let tree_iter_old = objs::TreeRefIter::from_bytes(&old.data);

    if let Some(arg) = args.new {
        let new = resolve_tree(&repo, arg.as_ref())?;
        let tree_iter_new = objs::TreeRefIter::from_bytes(&new.data);

        diff_two_trees(&repo, tree_iter_old, tree_iter_new, paths);
    } else {
        diff_with_disk(&repo, paths);
    }

    Ok(())
}

fn disk_newer_than_index(
    stat: &index::entry::Stat,
    path: &std::path::Path,
) -> Result<bool, BlameDiffError> {
    let fs_stat = std::fs::symlink_metadata(path)?;

    Ok((stat.mtime.secs as u64)
        < fs_stat
            .modified()?
            .duration_since(std::time::SystemTime::UNIX_EPOCH)?
            .as_secs())
}

fn diff_two_trees(
    repo: &Repository,
    tree_iter_old: objs::TreeRefIter,
    tree_iter_new: objs::TreeRefIter,
    paths: Vec<PathBuf>,
) -> Result<(), BlameDiffError> {
    let state = diff::tree::State::default();
    let mut recorder = diff::tree::Recorder::default();

    let changes = diff::tree::Changes::from(tree_iter_old);

    changes.needed_to_obtain(
        tree_iter_new,
        state,
        |id, buf| {
            let object = repo.try_find_object(id)?.ok_or(BlameDiffError::BadArgs)?;
            match object.kind {
                object::Kind::Tree => {
                    buf.clear();
                    buf.extend(object.data.iter());
                    Ok(objs::TreeRefIter::from_bytes(buf))
                }
                _ => Err(BlameDiffError::BadArgs),
            }
        },
        &mut recorder,
    )?;

    use diff::tree::recorder::Change::*;

    let iter = recorder.records.into_iter().filter(|c| match c {
        Addition { path, .. } | Deletion { path, .. } | Modification { path, .. } => {
            let p: &[u8] = path.as_ref();
            let p = PathBuf::from(std::str::from_utf8(p).expect("valid path"));

            paths.is_empty() || paths.contains(&p)
        }
    });

    for c in iter {
        match c {
            Addition {
                entry_mode: objs::tree::EntryMode::Blob,
                oid,
                path,
            } => diff_blob_with_null(repo, oid, path.as_ref(), false)?,
            Deletion {
                entry_mode: objs::tree::EntryMode::Blob,
                oid,
                path,
            } => diff_blob_with_null(repo, oid, path.as_ref(), true)?,
            Modification {
                previous_entry_mode: objs::tree::EntryMode::Blob,
                previous_oid,
                entry_mode: objs::tree::EntryMode::Blob,
                oid,
                path,
            } => diff_two_blobs(repo, previous_oid, oid, path.as_ref())?,
            x => {
                dbg!(x);
            }
        }
    }

    Ok(())
}

fn diff_with_disk(repo: &Repository, paths: Vec<PathBuf>) -> Result<(), BlameDiffError> {
    let index = repo.open_index().unwrap();
    for e in index.entries() {
        let p = e.path(&index).to_str().unwrap();
        let path = std::path::Path::new(p);

        if paths.is_empty() || paths.iter().any(|p| p == path) {
            if disk_newer_than_index(&e.stat, path)? {
                let disk_contents = std::fs::read_to_string(path)?;

                let blob = get_object(&repo, e.id, object::Kind::Blob)?;
                let blob_contents = std::str::from_utf8(&blob.data).unwrap();
                let input =
                    diff::blob::intern::InternedInput::new(blob_contents, disk_contents.as_str());

                let diff = diff::blob::diff(
                    diff::blob::Algorithm::Histogram,
                    &input,
                    UnifiedDiffBuilder::new(&input),
                );

                if !diff.is_empty() {
                    print!("--- a/{0}\n+++ b/{0}\n{1}", p, diff);
                }
            }
        }
    }

    Ok(())
}

fn diff_blob_with_null(
    repo: &Repository,
    oid: hash::ObjectId,
    path: &bstr::BStr,
    to_null: bool,
) -> Result<(), BlameDiffError> {
    let blob = get_object(repo, oid, object::Kind::Blob)?;
    let file = std::str::from_utf8(&blob.data).unwrap();

    let input = if to_null {
        println!("--- a/{}\n+++ /dev/null", path);
        diff::blob::intern::InternedInput::new(file, "")
    } else {
        println!("--- /dev/null\n+++ b/{}", path);
        diff::blob::intern::InternedInput::new("", file)
    };

    let diff = diff::blob::diff(
        diff::blob::Algorithm::Histogram,
        &input,
        UnifiedDiffBuilder::new(&input),
    );

    print!("{}", diff);

    Ok(())
}

fn diff_two_blobs(
    repo: &Repository,
    old_oid: hash::ObjectId,
    new_oid: hash::ObjectId,
    path: &bstr::BStr,
) -> Result<(), BlameDiffError> {
    let old = get_object(repo, old_oid, object::Kind::Blob)?;
    let new = get_object(repo, new_oid, object::Kind::Blob)?;

    let old_file = std::str::from_utf8(&old.data).expect("valid UTF-8");
    let new_file = std::str::from_utf8(&new.data).expect("valid UTF-8");

    let input = diff::blob::intern::InternedInput::new(old_file, new_file);

    let diff = diff::blob::diff(
        diff::blob::Algorithm::Histogram,
        &input,
        UnifiedDiffBuilder::new(&input),
    );

    println!("--- a/{0}\n+++ b/{0}\n{1}", path, diff);

    Ok(())
}
