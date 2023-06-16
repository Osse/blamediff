use std::path::Path;

mod common;

macro_rules! blame_test {
    ($test_name:ident, $range:literal) => {
        #[test]
        fn $test_name() {
            let r = gix::discover(".").unwrap();
            let blame = culpa::blame_file(&r, $range, false, &Path::new(common::FILE)).unwrap();
            let fasit = common::run_git_blame($range, &[]);
            common::compare($range, blame.blamed_lines(), fasit);
        }
    };
}

// These tests could be generated by a build.rs but that made running
// individual ones tedious and apparently rust-analyzer got confused.
//
// The lines below are generated by:
//
// git log --reverse --format='%h%x09%s' first-test | awk -F'\t' '{
//     printf("// %s: %s\nblame_test!(t%02d, \"%s\");\n", $1, $2, NR, $1);
//     for (i in hashes) {
//         printf("blame_test!(t%02d_%02d, \"%s..%s\");\n", NR, i, hashes[i], $1);
//     }
//     printf("\n")
//     hashes[NR] = $1
// }'

// 753d1db: Initial commit
blame_test!(t01, "753d1db");

// f28f649: Simple change
blame_test!(t02, "f28f649");
blame_test!(t02_01, "753d1db..f28f649");

// d3baed3: Removes more than it adds
blame_test!(t03, "d3baed3");
blame_test!(t03_01, "753d1db..d3baed3");
blame_test!(t03_02, "f28f649..d3baed3");

// 536a0f5: Adds more than it removes
blame_test!(t04, "536a0f5");
blame_test!(t04_01, "753d1db..536a0f5");
blame_test!(t04_02, "f28f649..536a0f5");
blame_test!(t04_03, "d3baed3..536a0f5");

// 6a30c80: Change on first line
blame_test!(t05, "6a30c80");
blame_test!(t05_01, "753d1db..6a30c80");
blame_test!(t05_02, "f28f649..6a30c80");
blame_test!(t05_03, "d3baed3..6a30c80");
blame_test!(t05_04, "536a0f5..6a30c80");

// 4d8a3c7: Multiple changes in one commit
blame_test!(t06, "4d8a3c7");
blame_test!(t06_01, "753d1db..4d8a3c7");
blame_test!(t06_02, "f28f649..4d8a3c7");
blame_test!(t06_03, "d3baed3..4d8a3c7");
blame_test!(t06_04, "536a0f5..4d8a3c7");
blame_test!(t06_05, "6a30c80..4d8a3c7");

// 2064b3c: Change on last line
blame_test!(t07, "2064b3c");
blame_test!(t07_01, "753d1db..2064b3c");
blame_test!(t07_02, "f28f649..2064b3c");
blame_test!(t07_03, "d3baed3..2064b3c");
blame_test!(t07_04, "536a0f5..2064b3c");
blame_test!(t07_05, "6a30c80..2064b3c");
blame_test!(t07_06, "4d8a3c7..2064b3c");

// 0e17ccb: Blank line in context
blame_test!(t08, "0e17ccb");
blame_test!(t08_01, "753d1db..0e17ccb");
blame_test!(t08_02, "f28f649..0e17ccb");
blame_test!(t08_03, "d3baed3..0e17ccb");
blame_test!(t08_04, "536a0f5..0e17ccb");
blame_test!(t08_05, "6a30c80..0e17ccb");
blame_test!(t08_06, "4d8a3c7..0e17ccb");
blame_test!(t08_07, "2064b3c..0e17ccb");

// 3be8265: Indent and overlap with previous change.
blame_test!(t09, "3be8265");
blame_test!(t09_01, "753d1db..3be8265");
blame_test!(t09_02, "f28f649..3be8265");
blame_test!(t09_03, "d3baed3..3be8265");
blame_test!(t09_04, "536a0f5..3be8265");
blame_test!(t09_05, "6a30c80..3be8265");
blame_test!(t09_06, "4d8a3c7..3be8265");
blame_test!(t09_07, "2064b3c..3be8265");
blame_test!(t09_08, "0e17ccb..3be8265");

// 8bf8780: Simple change but a bit bigger
blame_test!(t10, "8bf8780");
blame_test!(t10_01, "753d1db..8bf8780");
blame_test!(t10_02, "f28f649..8bf8780");
blame_test!(t10_03, "d3baed3..8bf8780");
blame_test!(t10_04, "536a0f5..8bf8780");
blame_test!(t10_05, "6a30c80..8bf8780");
blame_test!(t10_06, "4d8a3c7..8bf8780");
blame_test!(t10_07, "2064b3c..8bf8780");
blame_test!(t10_08, "0e17ccb..8bf8780");
blame_test!(t10_09, "3be8265..8bf8780");

// f7a3a57: Remove a lot
blame_test!(t11, "f7a3a57");
blame_test!(t11_01, "753d1db..f7a3a57");
blame_test!(t11_02, "f28f649..f7a3a57");
blame_test!(t11_03, "d3baed3..f7a3a57");
blame_test!(t11_04, "536a0f5..f7a3a57");
blame_test!(t11_05, "6a30c80..f7a3a57");
blame_test!(t11_06, "4d8a3c7..f7a3a57");
blame_test!(t11_07, "2064b3c..f7a3a57");
blame_test!(t11_08, "0e17ccb..f7a3a57");
blame_test!(t11_09, "3be8265..f7a3a57");
blame_test!(t11_10, "8bf8780..f7a3a57");

// 392db1b: Add a lot and blank lines
blame_test!(t12, "392db1b");
blame_test!(t12_01, "753d1db..392db1b");
blame_test!(t12_02, "f28f649..392db1b");
blame_test!(t12_03, "d3baed3..392db1b");
blame_test!(t12_04, "536a0f5..392db1b");
blame_test!(t12_05, "6a30c80..392db1b");
blame_test!(t12_06, "4d8a3c7..392db1b");
blame_test!(t12_07, "2064b3c..392db1b");
blame_test!(t12_08, "0e17ccb..392db1b");
blame_test!(t12_09, "3be8265..392db1b");
blame_test!(t12_10, "8bf8780..392db1b");
blame_test!(t12_11, "f7a3a57..392db1b");

// 1050bf8: Multiple changes in one commit again
blame_test!(t13, "1050bf8");
blame_test!(t13_01, "753d1db..1050bf8");
blame_test!(t13_02, "f28f649..1050bf8");
blame_test!(t13_03, "d3baed3..1050bf8");
blame_test!(t13_04, "536a0f5..1050bf8");
blame_test!(t13_05, "6a30c80..1050bf8");
blame_test!(t13_06, "4d8a3c7..1050bf8");
blame_test!(t13_07, "2064b3c..1050bf8");
blame_test!(t13_08, "0e17ccb..1050bf8");
blame_test!(t13_09, "3be8265..1050bf8");
blame_test!(t13_10, "8bf8780..1050bf8");
blame_test!(t13_11, "f7a3a57..1050bf8");
blame_test!(t13_12, "392db1b..1050bf8");
