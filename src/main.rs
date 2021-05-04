use git_diff::tree;
use git_odb::compound::Db;

fn main() {
    let db = git_odb::compound::Db::at(".git/objects");
    println!("db: {:?}", db);
}
