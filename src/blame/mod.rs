pub struct BloomData
{
}

impl BloomData {
    fn new() -> BloomData {
        BloomData  {}
    }
}

pub struct Entry
{
    lno: i32,
    num_lines: i32,
    suspect: Box<Origin>,
    s_lno: i32,
    score: u32,
    ignored: i32,
    unblamable: i32,
}

pub struct Origin
{
    refcnt: i32,
    // commit: &str,
    suspects: Vec<Entry>,
    // file: &str, //mmfile_t
    num_lines: i32,
    fingerprints: std::collections::HashMap<i32, i32>,
    // blob_id: &str, //struct object_id
    mode: u16,
    guilty: u8,
    // path: &str
}

pub struct Scoreboard<'a>
{
    final_: git_object::CommitRef<'a>,
    commits: std::collections::BinaryHeap<(u32, git_object::CommitRef<'a>)>, // struct prio_queue
    // repo: &str, // struct repository
    // revs: &str, // struct rev_info
    // path: &str,
    // final_buf: &[u8],

    ent: Vec<Entry>,

    // ignore_list: &str, // struct oidset

    num_lines: i32,
    lineno: i32, // *int ??

    num_read_blob: i32,
    num_get_patch: i32,
    num_commits: i32,

    move_score: u32,
    copy_score: u32,

    // contents_from: &str, // --contents-from, not interesting

    // reverse: i32, // --reverse flag, not interesting
    show_root: i32,
    xdl_opts: i32,
    no_whole_file_rename: i32,
    debug: i32,

    /* callbacks */
    on_sanity_fail: fn(scoreboard: &Scoreboard, i32) -> (),
    found_guilty_entry: fn(entry: &Entry, i32) -> (), // i32 is a void*

    found_guilty_entry_data: i32, // void*;
    bloom_data: BloomData,
}

impl<'a> Scoreboard<'a> {
    pub fn new(i: ScoreboardInit) -> Scoreboard<'a> {
        Scoreboard {
            final_: git_object::CommitRef::from_bytes(b"lol").unwrap(),
            commits: std::collections::BinaryHeap::<(u32, git_object::CommitRef)>::new(),
            ent: Vec::<Entry>::new(),
            num_lines: 0,
            lineno: 0,
            num_read_blob: 0,
            num_get_patch: 0,
            num_commits: 0,
            move_score: 0,
            copy_score: 0,
            show_root: 0,
            xdl_opts: 0,
            no_whole_file_rename: 0,
            debug: 0,
            on_sanity_fail: |sb, i| (),
            found_guilty_entry: |e, i| (),
            found_guilty_entry_data: 0,
            bloom_data: BloomData::new(),
        }
    }

    pub fn setup(&self, final_: &str, path: &str) {

    }
}

pub struct ScoreboardInit {
    pub final_: i32,
    pub path: std::path::PathBuf,
}
