use std::str::FromStr;

use culpa::*;

pub const FILE: &str = "lorem-ipsum.txt";

// Return list of strings in the format "SHA1 SP true/false SP <line contents>"
pub fn run_git_blame(revision: &str, parents: Parents) -> Vec<BlamedLine> {
    let blame_flags: &[&str] = match parents {
        Parents::All => &[],
        Parents::First => &["--first-parent"],
    };

    let output = std::process::Command::new("git")
        .args(["-C", ".."])
        .arg("blame")
        .args(blame_flags)
        .args(["--line-porcelain", revision, FILE])
        .output()
        .expect("able to run git rev-list")
        .stdout;

    let output = std::str::from_utf8(&output).expect("valid UTF-8");

    // Example output:
    //
    // 00 753d1dba0c677cdb2f32be664faaff55856ede66 4 4
    // 01 author Øystein Walle
    // 02 author-mail <you@example.com>
    // 03 author-time 1678823979
    // 04 author-tz +0100
    // 05 committer Øystein Walle
    // 06 committer-mail <oystwa@gmail.com>
    // 07 committer-time 1685450005
    // 08 committer-tz +0200
    // 09 summary Initial commit
    // 10 boundary
    // 11 filename lorem-ipsum.txt
    // 12         fermentum quam, varius vestibulum est nisi et ex. Sed luctus est eu odio
    //
    // 00 1050bf853e2209915f1864c3d7c9a22c599f8dc8 5 5 2
    // 01 author Øystein Walle
    // 02 author-mail <oystwa@gmail.com>
    // 03 author-time 1685440586
    // 04 author-tz +0200
    // 05 committer Øystein Walle
    // 06 committer-mail <oystwa@gmail.com>
    // 07 committer-time 1685450082
    // 08 committer-tz +0200
    // 09 summary Multiple changes in one commit again
    // 10 previous 392db1bdc8269623a3261e7340bfca3e61e209f4 lorem-ipsum.txt
    // 11 filename lorem-ipsum.txt
    // 12         Spenol er best i verden!

    output
        .split_terminator('\n')
        .collect::<Vec<_>>()
        .chunks(13)
        .map(|c| {
            let s = c[0].split_ascii_whitespace().collect::<Vec<&str>>();
            BlamedLine {
                id: gix::ObjectId::from_str(s[0]).expect("Valid id"),
                orig_line_no: s[1].parse().expect("valid"),
                line_no: s[2].parse().expect("valid"),
                boundary: c[10].starts_with("boundary"),
                line: c[12][1..].to_owned(),
            }
        })
        .collect()
}
