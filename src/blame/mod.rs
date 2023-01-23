#![allow(unused_must_use)]
#![allow(dead_code)]

pub struct BloomData {}

impl BloomData {
    fn new() -> BloomData {
        BloomData {}
    }
}

/*
 * Each group of lines is described by a blame_entry; it can be split
 * as we pass blame to the parents.  They are arranged in linked lists
 * kept as `suspects' of some unprocessed origin, or entered (when the
 * blame origin has been finalized) into the scoreboard structure.
 * While the scoreboard structure is only sorted at the end of
 * processing (according to final image line number), the lists
 * attached to an origin are sorted by the target line number.
 */
pub struct Entry {
    /* the first line of this group in the final image;
     * internally all line numbers are 0 based.
     */
    lno: i32,

    /* the first line of this group in the final image;
     * internally all line numbers are 0 based.
     */
    num_lines: i32,
    suspect: Box<Origin>,
    s_lno: i32,
    score: u32,
    ignored: i32,
    unblamable: i32,
}

/*
 * One blob in a commit that is being suspected
 */
pub struct Origin {
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

struct Commit;

impl Origin {
    fn get(commit: Commit, path: &str) -> Self {
        Self {
            refcnt: 1,
            suspects: vec![],
            num_lines: 0,
            fingerprints: std::collections::HashMap::new(),
            mode: 0,
            guilty: 0,
        }
    }
}

pub struct Scoreboard<'a> {
    final_: git_object::CommitRef<'a>,
    commits: std::collections::BinaryHeap<(u32, git_object::CommitRef<'a>)>, // struct prio_queue
    // repo: &str, // struct repository
    // revs: &str, // struct rev_info: Used for argument handling
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
    pub fn new(i: ScoreboardInit<'a>) -> Scoreboard<'a> {
        Scoreboard {
            final_: git_object::CommitRef::from_bytes(i.final_).unwrap(),
            commits: std::collections::BinaryHeap::<(u32, git_object::CommitRef)>::new(),
            ent: Vec::<Entry>::new(),
            num_lines: 0,
            lineno: 0,
            num_read_blob: 0,
            num_get_patch: 0,
            num_commits: 0,
            move_score: 20,
            copy_score: 40,
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

    pub fn blame_coalesce(&self) {}
    pub fn blame_sort_final(&self) {}
    pub fn blame_entry_score(&self, e: &Entry) -> u32 { 0 }
    pub fn assign_blame(&self, opt: i32) {}
    pub fn blame_nth_line(&self, lnu: usize) -> String { String::new() }
}

pub struct ScoreboardInit<'a> {
    pub final_: &'a [u8],
    pub path: std::path::PathBuf,
}
