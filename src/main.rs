#![allow(unused_must_use)]
#![allow(dead_code)]

mod diff;
use diff::UnifiedDiffBuilder;

use clap::Parser;

#[derive(Debug)]
enum BlameDiffError {
    BadArgs,
    DiscoverError(git_repository::discover::Error),
    PeelError(git_repository::object::peel::to_kind::Error),
    FindObject(git_odb::store::find::Error),
    DiffGeneration(git_diff::tree::changes::Error),
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
            fn from(_e: $e) -> Self {
                $b
            }
        }
    };
    ($e:ty, $b:expr, $f:literal) => {
        impl From<$e> for BlameDiffError {
            fn from(e: $e) -> Self {
                $b(e)
            }
        }
    };
}

make_error![git_hash::decode::Error, BlameDiffError::BadArgs];
make_error![
    git_repository::revision::spec::parse::Error,
    BlameDiffError::BadArgs
];
make_error![
    git_repository::discover::Error,
    BlameDiffError::DiscoverError,
    1
];
make_error![
    git_diff::tree::changes::Error,
    BlameDiffError::DiffGeneration,
    1
];
make_error![
    git_repository::object::peel::to_kind::Error,
    BlameDiffError::PeelError,
    1
];
make_error![git_odb::store::find::Error, BlameDiffError::FindObject, 1];

#[derive(Parser)]
#[command(author, version, about, long_about = None)]
struct Args {
    /// Old commit to diff
    #[arg(short, long)]
    old: bstr::BString,

    /// Old commit to diff
    #[arg(short, long)]
    new: bstr::BString,

    /// Paths to filter on
    paths: Vec<bstr::BString>,
}

fn resolve_tree<'a>(
    repo: &'a git_repository::Repository,
    object: &bstr::BString,
) -> Result<git_repository::Object<'a>, BlameDiffError> {
    let o: &bstr::BStr = object.as_ref();
    let old = repo.rev_parse(o)?.single().ok_or(BlameDiffError::BadArgs)?;
    repo.try_find_object(old)?
        .ok_or(BlameDiffError::BadArgs)?
        .peel_to_kind(git_repository::object::Kind::Tree)
        .map_err(|_| BlameDiffError::BadArgs)
}

fn main() -> Result<(), BlameDiffError> {
    let args = Args::parse();

    let repo = git_repository::discover(".")?;

    let old = resolve_tree(&repo, &args.old)?;
    let new = resolve_tree(&repo, &args.new)?;

    let tree_iter_old = git_object::TreeRefIter::from_bytes(&old.data);
    let tree_iter_new = git_object::TreeRefIter::from_bytes(&new.data);

    let state = git_diff::tree::State::default();
    let mut recorder = git_diff::tree::Recorder::default();

    let changes = git_diff::tree::Changes::from(tree_iter_old);

    changes.needed_to_obtain(
        tree_iter_new,
        state,
        |id, buf| {
            let object = repo.try_find_object(id)?.ok_or(BlameDiffError::BadArgs)?;
            match object.kind {
                git_repository::object::Kind::Tree => {
                    buf.clear();
                    buf.extend(object.data.iter());
                    Ok(git_object::TreeRefIter::from_bytes(buf))
                }
                _ => Err(BlameDiffError::BadArgs),
            }
        },
        &mut recorder,
    )?;

    print_patch(&repo, &recorder);

    Ok(())
}

fn print_patch(
    repo: &git_repository::Repository,
    recorder: &git_diff::tree::Recorder,
) -> Result<(), BlameDiffError> {
    use git_diff::tree::recorder::Change::*;

    for c in &recorder.records {
        match c {
            Addition {
                entry_mode: git_object::tree::EntryMode::Blob,
                oid,
                path,
            } => diff_blob_with_null(repo, oid, path, false)?,
            Addition { .. } => (),
            Deletion {
                entry_mode: git_object::tree::EntryMode::Blob,
                oid,
                path,
            } => diff_blob_with_null(repo, oid, path, true)?,
            Deletion { .. } => (),
            Modification {
                previous_entry_mode: git_object::tree::EntryMode::Blob,
                previous_oid,
                entry_mode: git_object::tree::EntryMode::Blob,
                oid,
                path,
            } => diff_blobs(repo, previous_oid, oid, path)?,
            Modification { .. } => (),
        }
    }

    Ok(())
}

fn get_blob<'a>(
    repo: &'a git_repository::Repository,
    oid: &git_hash::ObjectId,
) -> Result<git_repository::Object<'a>, BlameDiffError> {
    repo.try_find_object(*oid)?
        .ok_or(BlameDiffError::BadArgs)?
        .peel_to_kind(git_object::Kind::Blob)
        .map_err(|_| BlameDiffError::BadArgs)
}

fn diff_blob_with_null(
    repo: &git_repository::Repository,
    oid: &git_hash::ObjectId,
    path: &bstr::BString,
    to_null: bool,
) -> Result<(), BlameDiffError> {
    let blob = get_blob(repo, oid)?;
    let file = std::str::from_utf8(&blob.data).unwrap();

    let input = if to_null {
        println!("--- a/{}\n+++ /dev/null", path);
        git_diff::blob::intern::InternedInput::new(file, "")
    } else {
        println!("--- /dev/null\n+++ b/{}", path);
        git_diff::blob::intern::InternedInput::new("", file)
    };

    let diff = git_diff::blob::diff(
        git_diff::blob::Algorithm::Histogram,
        &input,
        UnifiedDiffBuilder::new(&input),
    );

    print!("{}", diff);

    Ok(())
}

fn diff_blobs(
    repo: &git_repository::Repository,
    old_oid: &git_hash::ObjectId,
    new_oid: &git_hash::ObjectId,
    path: &bstr::BString,
) -> Result<(), BlameDiffError> {
    let old = get_blob(repo, old_oid)?;
    let new = get_blob(repo, new_oid)?;

    let old_file = std::str::from_utf8(&old.data).unwrap();
    let new_file = std::str::from_utf8(&new.data).unwrap();

    let input = git_diff::blob::intern::InternedInput::new(old_file, new_file);

    let diff = git_diff::blob::diff(
        git_diff::blob::Algorithm::Histogram,
        &input,
        UnifiedDiffBuilder::new(&input),
    );

    println!("--- a/{0}\n+++ b/{0}\n{1}", path, diff);

    Ok(())
}
