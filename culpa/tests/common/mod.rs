use culpa::*;
use pretty_assertions::assert_eq;

pub const FILE: &str = "lorem-ipsum.txt";

// Return list of strings in the format "SHA1 SP <line contents>"
pub fn run_git_blame(revision: &str, args: &[&str]) -> Vec<String> {
    let output = std::process::Command::new("git")
        .args(["-C", "..", "blame"])
        .args(args)
        .args(["--line-porcelain", revision, FILE])
        .output()
        .expect("able to run git blame")
        .stdout;

    let output = std::str::from_utf8(&output).expect("valid UTF-8");

    output
        .split_terminator('\n')
        .collect::<Vec<_>>()
        .chunks(13)
        .map(|c| {
            format!(
                "{} {} {}",
                &c[0][..40],
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
        .map(|(bl, line)| format!("{} {} {}", bl.id.to_string(), bl.boundary, line))
        .collect();

    assert_eq!(fasit, blame);
}
