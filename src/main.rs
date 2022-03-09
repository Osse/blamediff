#![allow(unused_must_use)]
#![allow(dead_code)]
mod blame;

#[derive(Debug)]
enum BlameDiffError {
    BadArgs,
    DiscoverError(git_repository::discover::Error),
    PeelError(git_repository::object::peel::to_kind::Error),
    FindObject(git_odb::store::find::Error),
    DiffGeneration(git_diff::tree::changes::Error),
}

macro_rules! error {
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
    }
}

error![git_hash::decode::Error, BlameDiffError::BadArgs];
error![git_repository::discover::Error, BlameDiffError::DiscoverError, 1];
error![git_diff::tree::changes::Error, BlameDiffError::DiffGeneration, 1];
error![git_repository::object::peel::to_kind::Error, BlameDiffError::PeelError, 1];
error![git_odb::store::find::Error, BlameDiffError::FindObject, 1];

fn main() -> Result<(), BlameDiffError> {
    let args = std::env::args().collect::<Vec<String>>();

    if args.len() < 3 {
        return Err(BlameDiffError::BadArgs);
    }

    let old = git_repository::ObjectId::from_hex(args[1].as_bytes())?;
    let new = git_repository::ObjectId::from_hex(args[2].as_bytes())?;

    let repo = git_repository::discover(".")?;

    let old =
        repo.try_find_object(old)
        .expect("h.try_find_object() failed")
        .expect("No object found")
        .peel_to_kind(git_repository::object::Kind::Tree)?;

    let new =
        repo.try_find_object(new)
        .expect("repo.try_find_object() failed")
        .expect("No object found")
        .peel_to_kind(git_repository::object::Kind::Tree)?;
    dbg!(&old);
    dbg!(&new);

    let tree_iter_old = git_object::TreeRefIter::from_bytes(&old.data);
    let tree_iter_new = git_object::TreeRefIter::from_bytes(&new.data);

    let state = git_diff::tree::State::default();
    let mut recorder = git_diff::tree::Recorder::default();

    let changes = git_diff::tree::Changes::from(tree_iter_old);

    changes.needed_to_obtain(
        tree_iter_new,
        state,
        |id, buf| {
            let object = repo
                .try_find_object(id)
                .expect("repo.try_find_object() failed")
                .expect("No object found");
            match object.kind {
                git_repository::object::Kind::Tree => {
                    buf.clear();
                    buf.extend(object.data.iter());
                    Some(git_object::TreeRefIter::from_bytes(buf))
                }
                _ => None,
            }
        },
        &mut recorder,
    )?;

    print_patch(&repo, &recorder);

    let mut entries = Vec::<blame::Entry>::new();

    let i = blame::ScoreboardInit {
        final_: 2,
        path: std::path::PathBuf::from("Cargo.toml")
    };

    let mut sb = blame::Scoreboard::new(i);

    let mut blame_suspects = std::collections::HashMap::<&str, blame::Origin>::new();

    Ok(())
}

fn print_patch(repo: &git_repository::Repository, recorder: &git_diff::tree::Recorder) -> Result<(), BlameDiffError> {
    for c in &recorder.records {
        match c {
            git_diff::tree::recorder::Change::Addition { .. } => (),
            git_diff::tree::recorder::Change::Deletion { .. } => (),
            git_diff::tree::recorder::Change::Modification {
                previous_entry_mode: git_object::tree::EntryMode::Blob,
                previous_oid,
                entry_mode: git_object::tree::EntryMode::Blob,
                oid,
                path,
            } => diff_blobs(repo, previous_oid, oid, path)?,
            git_diff::tree::recorder::Change::Modification { .. } => (),
        }
    }

    Ok(())
}

fn diff_blobs(
    repo: &git_repository::Repository,
    old_oid: &git_hash::ObjectId,
    new_oid: &git_hash::ObjectId,
    path: &bstr::BString,
) -> Result<(), BlameDiffError> {
    let old_blob = repo
        .try_find_object(*old_oid)?
        .expect("None");

    let new_blob = repo
        .try_find_object(*new_oid)?
        .expect("None");

    if old_blob.kind != git_repository::object::Kind::Blob || 
    new_blob.kind != git_repository::object::Kind::Blob {
        return Err(BlameDiffError::BadArgs);
    }

    let diff = similar::TextDiff::from_lines(&old_blob.data, &new_blob.data);
    let unified_diff = diff.unified_diff();

    use colored::Colorize; // "foobar".red()

    println!(
        "{}",
        format!(
            "diff --git a/{0} b/{0}\nindex {1}..{2} 100644\n--- a/{0}\n+++ b/{0}",
            path.to_string(),
            &old_oid.to_hex(),
            &new_oid.to_hex()
        )
        .bold()
    );

    for hunk in unified_diff.iter_hunks() {
        println!("{}", format!("{}", hunk.header()).cyan());
        for change in hunk.iter_changes() {
            match change.tag() {
                similar::ChangeTag::Delete => print!("{}", format!("-{}", change).red()),
                similar::ChangeTag::Insert => print!("{}", format!("+{}", change).green()),
                similar::ChangeTag::Equal => print!(" {}", change),
            };
        }
    }

    Ok(())
}
