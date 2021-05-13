fn get_tree_iter2<'a>(
    db: &git_odb::compound::Db,
    buffer: &'a mut Vec<u8>,
    oid: &git_hash::oid,
) -> Option<git_object::immutable::tree::TreeIter<'a>> {
    let object = db
        .find(&oid, buffer, &mut git_odb::pack::cache::Never)
        .unwrap()
        .unwrap();

    object.into_tree_iter()
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

    let mut db = git_odb::compound::Db::at(".git/objects").unwrap();

    let mut buffer = Vec::<u8>::new();
    let tree_iter = get_tree_iter2(&db, &mut buffer, &old).unwrap();

    let mut buffer2 = Vec::<u8>::new();
    let tree_iter_other = get_tree_iter2(&db, &mut buffer2, &new).unwrap();

    let state = git_diff::tree::State::<usize>::default();
    let mut recorder = git_diff::tree::Recorder::default();

    let changes = git_diff::tree::Changes::from(tree_iter);

    changes.needed_to_obtain(
        tree_iter_other,
        state,
        |id, buf| get_tree_iter2(&db, buf, id),
        &mut recorder,
    );

    print_patch(&db, &recorder);
}

fn print_patch(db: &git_odb::compound::Db, recorder: &git_diff::tree::Recorder) {
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
    db: &git_odb::compound::Db,
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
