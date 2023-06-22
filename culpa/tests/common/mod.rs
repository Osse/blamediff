use culpa::*;
use pretty_assertions::assert_eq;

pub const FILE: &str = "lorem-ipsum.txt";

// Return list of strings in the format "SHA1 SP true/false SP <line contents>"
pub fn run_git_blame(revision: &str, args: &[&str]) -> Vec<String> {
    let output = std::process::Command::new("git")
        .args(["-C", "..", "blame"])
        .args(args)
        .args(["--line-porcelain", revision, FILE])
        .output()
        .expect("able to run git blame")
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
            format!(
                "{} {} {} {} {}",
                s[0],
                s[2],
                1, //TODO
                c[10].starts_with("boundary"),
                &c[12][1..]
            )
        })
        .collect()
}

pub fn compare(range: &str, blame: Vec<BlamedLine>, fasit: Vec<String>) {
    let sha1 = range.find('.').map(|p| &range[p + 2..]).unwrap_or(range);
    let blob = sha1.to_string() + ":" + FILE;

    let output = std::process::Command::new("git")
        .args(["show", &blob])
        .output()
        .expect("able to run git show")
        .stdout;

    let contents = std::str::from_utf8(&output).expect("valid UTF-8");

    // Create a Vec of Strings similar to the one obtained from git itself.
    // This with pretty assertions makes it much more pleasant to debug
    let blame: Vec<String> = blame
        .into_iter()
        .zip(contents.lines())
        .map(|(bl, line)| {
            format!(
                "{} {} {} {} {}",
                bl.id.to_string(),
                bl.line_no,
                bl.orig_line_no,
                bl.boundary,
                line
            )
        })
        .collect();

    assert_eq!(fasit, blame);
}
