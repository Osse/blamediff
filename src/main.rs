fn get_tree<'a>(
    db: &git_odb::compound::Store,
    buffer: &'a mut Vec<u8>,
    oid: &git_hash::oid,
) -> Option<git_odb::data::Object<'a>> {
    let mut b = Vec::<u8>::new();
    let object = db
        .find(&oid, &mut b, &mut git_odb::pack::cache::Never)
        .expect("db.find() failed")
        .expect("No object found");

    match object.kind {
        git_object::Kind::Commit => {
            let c = object.decode().unwrap().into_commit().unwrap();
            get_tree(db, buffer, &c.tree())
        }
        git_object::Kind::Tree => {
            *buffer = b;
            Some(git_odb::data::Object::new(git_object::Kind::Tree, buffer))
        }
        _ => None,
    }
}

fn arg_to_obj(s: &String) -> git_hash::ObjectId {
    git_hash::ObjectId::from_hex(s.as_bytes()).unwrap()
}

fn main() {
    let args = std::env::args().collect::<Vec<String>>();

    if args.len() < 3 {
        eprintln!("Need two treeishes");
        return;
    }

    let old = arg_to_obj(&args[1]);
    let new = arg_to_obj(&args[2]);

    let db = git_odb::compound::Store::at(".git/objects").unwrap();

    let mut buf_old = Vec::<u8>::new();
    let mut buf_new = Vec::<u8>::new();

    get_tree(&db, &mut buf_old, &old).expect("get_tree failed");
    get_tree(&db, &mut buf_new, &new).expect("get_tree failed");

    let tree_iter_old = git_object::immutable::tree::TreeIter::from_bytes(&buf_old);
    let tree_iter_new = git_object::immutable::tree::TreeIter::from_bytes(&buf_new);

    let state = git_diff::tree::State::default();
    let mut recorder = git_diff::tree::Recorder::default();

    let changes = git_diff::tree::Changes::from(tree_iter_old);

    changes.needed_to_obtain(
        tree_iter_new,
        state,
        |id, buf| {
            let object = db
                .find(&id, buf, &mut git_odb::pack::cache::Never)
                .expect("db.find() failed")
                .expect("No object found");

            object.into_tree_iter()
        },
        &mut recorder,
    );

    print_patch(&db, &recorder);
}

fn print_patch(db: &git_odb::compound::Store, recorder: &git_diff::tree::Recorder) {
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
            } => diff_blobs(db, previous_oid, oid, path),
            git_diff::tree::recorder::Change::Modification { .. } => (),
        }
    }
}

fn diff_blobs(
    db: &git_odb::compound::Store,
    old_oid: &git_hash::ObjectId,
    new_oid: &git_hash::ObjectId,
    path: &bstr::BString,
) {
    let mut old_buf = Vec::<u8>::new();
    let old_blob = db
        .find(&old_oid, &mut old_buf, &mut git_odb::pack::cache::Never)
        .expect("Bad result")
        .expect("None")
        .decode()
        .expect("Could not decode")
        .into_blob()
        .expect("into_blob failed");

    let mut new_buf = Vec::<u8>::new();
    let new_blob = db
        .find(&new_oid, &mut new_buf, &mut git_odb::pack::cache::Never)
        .expect("Bad result")
        .expect("None")
        .decode()
        .expect("Could not decode")
        .into_blob()
        .expect("into_blob failed");

    let diff = similar::TextDiff::from_lines(old_blob.data, new_blob.data);
    let unified_diff = diff.unified_diff();

    use colored::Colorize; // "foobar".red()

    println!(
        "{}",
        format!(
            "diff --git a/{0} b/{0}\nindex {1}..{2} 100644\n--- a/{0}\n+++ b/{0}",
            path.to_string(),
            &old_oid.to_sha1_hex_string()[0..7],
            &new_oid.to_sha1_hex_string()[0..7]
        )
        .bold()
    );

    for hunk in unified_diff.iter_hunks() {
        println!("{}", format!("{}", hunk.header()).cyan());
        for change in hunk.iter_changes() {
            match change.tag() {
                similar::ChangeTag::Delete => print!("{}", format!("-{}", change).red()),
                similar::ChangeTag::Insert => print!("{}", format!("-{}", change).green()),
                similar::ChangeTag::Equal => print!(" {}", change),
            };
        }
    }
}
