use std::env;
use std::ffi::OsStr;
use std::io::Write;
use std::path::Path;

const TAG: &str = "first-test";

fn handle_rerun(out_dir: &OsStr) -> std::io::Result<()> {
    let output = std::process::Command::new("git")
        .args(["rev-parse", TAG])
        .output()
        .expect("run Git in build.rs")
        .stdout;
    let output = std::str::from_utf8(&output).expect("valid UTF-8");

    let hash_file = Path::new(&out_dir).join("hash.txt");

    let update = || -> std::io::Result<()> {
        let f = std::fs::File::create(&hash_file).expect("able to open file");
        write!(&f, "{}", output)?;
        Ok(())
    };

    match std::fs::read_to_string(&hash_file) {
        Ok(s) if &s != output => update(),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => update(),
        _ => Ok(()),
    }?;

    println!("cargo:rerun-if-changed={}", hash_file.display());

    Ok(())
}

fn main() -> std::io::Result<()> {
    let out_dir = env::var_os("OUT_DIR").unwrap();
    handle_rerun(&out_dir)?;
    generate(&out_dir)?;
    Ok(())
}

fn generate(out_dir: &OsStr) -> std::io::Result<()> {
    let dest_path = Path::new(&out_dir).join("tests.rs");

    let f = std::fs::File::create(dest_path).expect("able to open file");

    let output = std::process::Command::new("git")
        .args(["log", "--reverse", "--format=%h%x09%s", TAG])
        .output()
        .expect("run Git in build.rs")
        .stdout;

    let output = std::str::from_utf8(&output)
        .expect("sensible output")
        .lines()
        .map(|l| l.split_once('\t').expect("tab"))
        .collect::<Vec<(&str, &str)>>();

    for (idx, (hash, message)) in output.iter().enumerate() {
        writeln!(
            &f,
            "blame_test!(t{:02}_{}, \"{}\");",
            idx + 1,
            hash,
            message,
        )?;

        for (prev_idx, (prev_hash, prev_msg)) in output[0..idx].iter().enumerate() {
            writeln!(
                &f,
                "blame_test!(t{:02}_{:02}_{}, \"{}\", \"{} - {}\");",
                idx + 1,
                prev_idx + 1,
                hash,
                prev_hash,
                prev_msg,
                message,
            )?;
        }
        writeln!(&f, "")?;
    }

    Ok(())
}
