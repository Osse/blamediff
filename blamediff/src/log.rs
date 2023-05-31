#![allow(unused_must_use)]
#![allow(dead_code)]
#![allow(unused_imports)]

use gix::{bstr, discover, hash, index, object, objs, Object, Repository};

pub fn log(paths: &[std::path::PathBuf]) {
    let path = &paths[0];
    let repo = discover(".").unwrap();

    let head = repo.rev_parse("HEAD").unwrap().single().unwrap();

    let mut iter = repo
        .rev_walk(std::iter::once(head))
        .all()
        .unwrap()
        .peekable();

    while let Some(c_id) = iter.next() {
        let c_id = c_id.unwrap();

        let c = repo
            .find_object(c_id)
            .unwrap()
            .peel_to_kind(object::Kind::Commit)
            .unwrap()
            .into_commit();

        let e = c.tree().unwrap().lookup_entry_by_path(path).unwrap();

        if let Some(e) = e {
            if let Some(aa) = iter.peek() {
                let aa = aa.as_ref().unwrap();

                let cc = repo
                    .find_object(*aa)
                    .unwrap()
                    .peel_to_kind(object::Kind::Commit)
                    .unwrap()
                    .into_commit();

                let ee = cc.tree().unwrap().lookup_entry_by_path(path).unwrap();

                if let Some(ee) = ee {
                    if e.object_id() != ee.object_id() {
                        println!("{} {}", c.id, c.message().unwrap().summary())
                    }
                }
            } else {
                println!("{} {}", c.id, c.message().unwrap().summary());
            }
        }
    }
}
