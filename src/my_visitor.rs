pub struct MyVisitor<'a> {
    db: &'a git_odb::compound::Db,
    path_components: Vec<bstr::BString>,
    counter: usize,
}

impl git_diff::tree::Visit for MyVisitor<'_> {
    type PathId = usize;

    fn set_current_path(&mut self, path: Self::PathId) {
        println!("set_current_path: {:?}", path);
    }

    fn push_tracked_path_component(&mut self, component: &bstr::BStr) -> Self::PathId {
        self.path_components.push(bstr::BString::from(component));
        // println!("push_tracked_path_component: {}", component);
    }

    fn push_path_component(&mut self, component: &bstr::BStr) {
        self.path_components.push(bstr::BString::from(component));
        // println!("push_path_component: {}", component);
    }

    fn pop_path_component(&mut self) {
        self.path_components.pop();
        // println!("pop_path_component");
    }

    fn visit(&mut self, change: git_diff::tree::visit::Change) -> git_diff::tree::visit::Action {
        match change {
            git_diff::tree::visit::Change::Addition { .. } => self.print_addition(),
            git_diff::tree::visit::Change::Deletion { .. } => self.print_deletion(),
            git_diff::tree::visit::Change::Modification {
                previous_entry_mode: git_object::tree::EntryMode::Blob,
                previous_oid,
                entry_mode: git_object::tree::EntryMode::Blob,
                oid,
            } => self.diff_blobs(previous_oid, oid),
            git_diff::tree::visit::Change::Modification { .. } => self.print_modification(),
        };

        git_diff::tree::visit::Action::Continue
    }
}

impl<'a> MyVisitor<'a> {
    pub fn new(db: &'a git_odb::compound::Db) -> MyVisitor<'a> {
        MyVisitor { db, path_components: Vec::<bstr::BString>::new(), counter: 0 }
    }

    fn print_addition(&self) {
        println!("print_addition");
    }

    fn print_deletion(&self) {
        println!("print_deletion");
    }

    fn current_path(&self) -> String {
        format!("{:?}", self.path_components)
    }

    fn diff_blobs(&self, old_oid: git_hash::ObjectId, new_oid: git_hash::ObjectId) {
        let mut old_buf = Vec::<u8>::new();
        let old_blob = self
            .db
            .find(&old_oid, &mut old_buf, &mut git_odb::pack::cache::Never)
            .expect("Bad result")
            .expect("None")
            .decode()
            .expect("Could not decode")
            .into_blob()
            .expect("into_blob failed");

        let mut new_buf = Vec::<u8>::new();
        let new_blob = self
            .db
            .find(&new_oid, &mut new_buf, &mut git_odb::pack::cache::Never)
            .expect("Bad result")
            .expect("None")
            .decode()
            .expect("Could not decode")
            .into_blob()
            .expect("into_blob failed");

        let diff = similar::TextDiff::from_lines(old_blob.data, new_blob.data);
        let mut unified_diff = diff.unified_diff();

        let p = self.current_path();
        println!("diff --git a/{} b/{}", p, p);
        println!("index {}..{} 100644", 1, 2);
        println!("--- a/{}", p);
        println!("--- b/{}", p);

        for hunk in unified_diff.iter_hunks() {
            println!("{}", hunk.header());
            for change in hunk.iter_changes() {
                let sign = match change.tag() {
                    similar::ChangeTag::Delete => "-",
                    similar::ChangeTag::Insert => "+",
                    similar::ChangeTag::Equal => " ",
                };
                print!("{}{}", sign, change);
            }
        }


        // for change in diff.iter_all_changes() {
        //     let sign = match change.tag() {
        //         similar::ChangeTag::Delete => "-",
        //         similar::ChangeTag::Insert => "+",
        //         similar::ChangeTag::Equal => " ",
        //     };
        //     print!("{}{}", sign, change);
        // }
    }

    fn print_modification(&self) {
        println!("print_modification");
    }
}
