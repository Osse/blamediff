mod my_visitor;
use my_visitor::MyVisitor;

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

fn get_tree_iter<'a>(
    db: &git_odb::compound::Db,
    buffer: &'a mut Vec<u8>,
    sha1: &[u8; 40],
) -> git_object::immutable::tree::TreeIter<'a> {
    let oid = git_hash::ObjectId::from_hex(sha1).unwrap();
    get_tree_iter2(db, buffer, &oid).expect("wtf")
}

fn main() {
    let mut db = git_odb::compound::Db::at(".git/objects").unwrap();

    let mut buffer = Vec::<u8>::new();
    let tree_iter = get_tree_iter(
        &db,
        &mut buffer,
        b"4f40f3e7d0f04837c59e67a7b2c7b22a5637d4a9",
    );

    let mut buffer2 = Vec::<u8>::new();
    let tree_iter_other = get_tree_iter(
        &db,
        &mut buffer2,
        b"33d1d2514525ec8fddf4e583ac9a300eae3bd460",
    );

    let state = git_diff::tree::State::<()>::default();
    let mut my_visitor = MyVisitor::new(&db);

    let changes = git_diff::tree::Changes::from(tree_iter);

    changes.needed_to_obtain(
        tree_iter_other,
        state,
        |id, buf| get_tree_iter2(&db, buf, id),
        &mut my_visitor,
    );
}
