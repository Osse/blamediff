fn get_tree_iter<'a>(
    db: &mut git_odb::compound::Db,
    buffer: &'a mut Vec<u8>,
    sha1: &[u8; 40],
) -> git_object::immutable::tree::TreeIter<'a> {
    let oid = git_hash::ObjectId::from_hex(sha1).unwrap();

    let object = db
        .find(&oid, buffer, &mut git_odb::pack::cache::Never)
        .unwrap()
        .unwrap();

    object.into_tree_iter().unwrap()
}

fn main() {
    let mut buffer = Vec::<u8>::new();

    let mut db = git_odb::compound::Db::at(".git/objects").unwrap();

    let tree_iter = get_tree_iter(&mut db, &mut buffer, b"5ad9d8655bc9ecd7363e8350ffe54b85b2fc0c69");
    let changes = git_diff::tree::Changes::from(tree_iter);
}
