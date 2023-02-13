#![allow(unused_must_use)]
#![allow(dead_code)]

mod diffprinter;

use diffprinter::UnifiedDiffBuilder;
use git_repository::bstr;
use git_repository::bstr::ByteSlice;

use clap::Parser;

use git_repository::{
    diff, discover, hash, index, object, objs, odb, revision, Object, Repository,
};

#[derive(Debug)]
enum BlameDiffError {
    BadArgs,
    Decode(hash::decode::Error),
    DiscoverError(discover::Error),
    PeelError(object::peel::to_kind::Error),
    FindObject(odb::store::find::Error),
    DiffGeneration(diff::tree::changes::Error),
    Io(std::io::Error),
    SystemTime(std::time::SystemTimeError),
    Parse(revision::spec::parse::Error),
}

impl std::fmt::Display for BlameDiffError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            BlameDiffError::BadArgs => write!(f, "Bad args"),
            _ => write!(f, "other badness"),
        }
    }
}

impl std::error::Error for BlameDiffError {}

macro_rules! make_error {
    ($e:ty, $b:expr) => {
        impl From<$e> for BlameDiffError {
            fn from(e: $e) -> Self {
                $b(e)
            }
        }
    };
}

make_error![hash::decode::Error, BlameDiffError::Decode];
make_error![revision::spec::parse::Error, BlameDiffError::Parse];
make_error![discover::Error, BlameDiffError::DiscoverError];
make_error![diff::tree::changes::Error, BlameDiffError::DiffGeneration];
make_error![object::peel::to_kind::Error, BlameDiffError::PeelError];
make_error![odb::store::find::Error, BlameDiffError::FindObject];
make_error![std::io::Error, BlameDiffError::Io];
make_error![std::time::SystemTimeError, BlameDiffError::SystemTime];

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
    paths: Vec<bstr::BString>,
}

fn resolve_tree<'a>(
    repo: &'a Repository,
    object: &bstr::BStr,
) -> Result<Object<'a>, BlameDiffError> {
    let object = repo
        .rev_parse(object)?
        .single()
        .ok_or(BlameDiffError::BadArgs)?;
    repo.try_find_object(object)?
        .ok_or(BlameDiffError::BadArgs)?
        .peel_to_kind(object::Kind::Tree)
        .map_err(|_| BlameDiffError::BadArgs)
}

fn main() -> Result<(), BlameDiffError> {
    let args = Args::parse();

    let repo = discover(".")?;

    let old = args.old.unwrap_or(bstr::BString::from("HEAD"));
    let old = resolve_tree(&repo, old.as_ref())?;
    let tree_iter_old = objs::TreeRefIter::from_bytes(&old.data);

    match args.new {
        Some(arg) => {
            let new = resolve_tree(&repo, arg.as_ref())?;
            let tree_iter_new = objs::TreeRefIter::from_bytes(&new.data);
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

            print_patch(&repo, &recorder);
        }
        None => {
            let index = repo.open_index().unwrap();
            for e in index.entries() {
                let p = e.path(&index).to_str().unwrap();
                let path = std::path::Path::new(p);

                if disk_newer_than_index(&e.stat, path)? {
                    let disk_contents = std::fs::read_to_string(path)?;

                    let blob = get_blob(&repo, &e.id)?;
                    let blob_contents = std::str::from_utf8(&blob.data).unwrap();
                    let input = diff::blob::intern::InternedInput::new(
                        blob_contents,
                        disk_contents.as_str(),
                    );

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

fn print_patch(repo: &Repository, recorder: &diff::tree::Recorder) -> Result<(), BlameDiffError> {
    use diff::tree::recorder::Change::*;

    for c in &recorder.records {
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
            x => { dbg!(x); }
        }
    }

    Ok(())
}

fn get_blob<'a>(repo: &'a Repository, oid: &hash::ObjectId) -> Result<Object<'a>, BlameDiffError> {
    repo.try_find_object(*oid)?
        .ok_or(BlameDiffError::BadArgs)?
        .peel_to_kind(object::Kind::Blob)
        .map_err(|_| BlameDiffError::BadArgs)
}

fn diff_blob_with_null(
    repo: &Repository,
    oid: &hash::ObjectId,
    path: &bstr::BStr,
    to_null: bool,
) -> Result<(), BlameDiffError> {
    let blob = get_blob(repo, oid)?;
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
    old_oid: &hash::ObjectId,
    new_oid: &hash::ObjectId,
    path: &bstr::BStr,
) -> Result<(), BlameDiffError> {
    let old = get_blob(repo, old_oid)?;
    let new = get_blob(repo, new_oid)?;

    let old_file = std::str::from_utf8(&old.data).unwrap();
    let new_file = std::str::from_utf8(&new.data).unwrap();

    let input = diff::blob::intern::InternedInput::new(old_file, new_file);

    let diff = diff::blob::diff(
        diff::blob::Algorithm::Histogram,
        &input,
        UnifiedDiffBuilder::new(&input),
    );

    println!("--- a/{0}\n+++ b/{0}\n{1}", path, diff);

    Ok(())
}
