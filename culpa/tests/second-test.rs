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
/*
git log --reverse --topo-order --format='%h%x09%s' second-test | awk -F'\t' '{
    printf("// %1$s: %2$s\n", $1, $2)
    printf("blame_test!(t%2$02d_%1$s, \"%1$s\");\n", $1, NR);
    for (i in hashes) {
        printf("blame_test!(t%2$02d_%3$02d_%4$s_%1$s, \"%4$s..%1$s\");\n", $1, NR, i, hashes[i]);
    }
    printf("\n")
    hashes[NR] = $1
}' >> culpa/tests/second-test.rs
*/

// 753d1db: Initial commit
blame_test!(t01_753d1db, "753d1db");

// f28f649: Simple change
blame_test!(t02_f28f649, "f28f649");
blame_test!(t02_01_753d1db_f28f649, "753d1db..f28f649");

// d3baed3: Removes more than it adds
blame_test!(t03_d3baed3, "d3baed3");
blame_test!(t03_01_753d1db_d3baed3, "753d1db..d3baed3");
blame_test!(t03_02_f28f649_d3baed3, "f28f649..d3baed3");

// 536a0f5: Adds more than it removes
blame_test!(t04_536a0f5, "536a0f5");
blame_test!(t04_01_753d1db_536a0f5, "753d1db..536a0f5");
blame_test!(t04_02_f28f649_536a0f5, "f28f649..536a0f5");
blame_test!(t04_03_d3baed3_536a0f5, "d3baed3..536a0f5");

// 6a30c80: Change on first line
blame_test!(t05_6a30c80, "6a30c80");
blame_test!(t05_01_753d1db_6a30c80, "753d1db..6a30c80");
blame_test!(t05_02_f28f649_6a30c80, "f28f649..6a30c80");
blame_test!(t05_03_d3baed3_6a30c80, "d3baed3..6a30c80");
blame_test!(t05_04_536a0f5_6a30c80, "536a0f5..6a30c80");

// 4d8a3c7: Multiple changes in one commit
blame_test!(t06_4d8a3c7, "4d8a3c7");
blame_test!(t06_01_753d1db_4d8a3c7, "753d1db..4d8a3c7");
blame_test!(t06_02_f28f649_4d8a3c7, "f28f649..4d8a3c7");
blame_test!(t06_03_d3baed3_4d8a3c7, "d3baed3..4d8a3c7");
blame_test!(t06_04_536a0f5_4d8a3c7, "536a0f5..4d8a3c7");
blame_test!(t06_05_6a30c80_4d8a3c7, "6a30c80..4d8a3c7");

// 2064b3c: Change on last line
blame_test!(t07_2064b3c, "2064b3c");
blame_test!(t07_01_753d1db_2064b3c, "753d1db..2064b3c");
blame_test!(t07_02_f28f649_2064b3c, "f28f649..2064b3c");
blame_test!(t07_03_d3baed3_2064b3c, "d3baed3..2064b3c");
blame_test!(t07_04_536a0f5_2064b3c, "536a0f5..2064b3c");
blame_test!(t07_05_6a30c80_2064b3c, "6a30c80..2064b3c");
blame_test!(t07_06_4d8a3c7_2064b3c, "4d8a3c7..2064b3c");

// 0e17ccb: Blank line in context
blame_test!(t08_0e17ccb, "0e17ccb");
blame_test!(t08_01_753d1db_0e17ccb, "753d1db..0e17ccb");
blame_test!(t08_02_f28f649_0e17ccb, "f28f649..0e17ccb");
blame_test!(t08_03_d3baed3_0e17ccb, "d3baed3..0e17ccb");
blame_test!(t08_04_536a0f5_0e17ccb, "536a0f5..0e17ccb");
blame_test!(t08_05_6a30c80_0e17ccb, "6a30c80..0e17ccb");
blame_test!(t08_06_4d8a3c7_0e17ccb, "4d8a3c7..0e17ccb");
blame_test!(t08_07_2064b3c_0e17ccb, "2064b3c..0e17ccb");

// 3be8265: Indent and overlap with previous change.
blame_test!(t09_3be8265, "3be8265");
blame_test!(t09_01_753d1db_3be8265, "753d1db..3be8265");
blame_test!(t09_02_f28f649_3be8265, "f28f649..3be8265");
blame_test!(t09_03_d3baed3_3be8265, "d3baed3..3be8265");
blame_test!(t09_04_536a0f5_3be8265, "536a0f5..3be8265");
blame_test!(t09_05_6a30c80_3be8265, "6a30c80..3be8265");
blame_test!(t09_06_4d8a3c7_3be8265, "4d8a3c7..3be8265");
blame_test!(t09_07_2064b3c_3be8265, "2064b3c..3be8265");
blame_test!(t09_08_0e17ccb_3be8265, "0e17ccb..3be8265");

// 8bf8780: Simple change but a bit bigger
blame_test!(t10_8bf8780, "8bf8780");
blame_test!(t10_01_753d1db_8bf8780, "753d1db..8bf8780");
blame_test!(t10_02_f28f649_8bf8780, "f28f649..8bf8780");
blame_test!(t10_03_d3baed3_8bf8780, "d3baed3..8bf8780");
blame_test!(t10_04_536a0f5_8bf8780, "536a0f5..8bf8780");
blame_test!(t10_05_6a30c80_8bf8780, "6a30c80..8bf8780");
blame_test!(t10_06_4d8a3c7_8bf8780, "4d8a3c7..8bf8780");
blame_test!(t10_07_2064b3c_8bf8780, "2064b3c..8bf8780");
blame_test!(t10_08_0e17ccb_8bf8780, "0e17ccb..8bf8780");
blame_test!(t10_09_3be8265_8bf8780, "3be8265..8bf8780");

// f7a3a57: Remove a lot
blame_test!(t11_f7a3a57, "f7a3a57");
blame_test!(t11_01_753d1db_f7a3a57, "753d1db..f7a3a57");
blame_test!(t11_02_f28f649_f7a3a57, "f28f649..f7a3a57");
blame_test!(t11_03_d3baed3_f7a3a57, "d3baed3..f7a3a57");
blame_test!(t11_04_536a0f5_f7a3a57, "536a0f5..f7a3a57");
blame_test!(t11_05_6a30c80_f7a3a57, "6a30c80..f7a3a57");
blame_test!(t11_06_4d8a3c7_f7a3a57, "4d8a3c7..f7a3a57");
blame_test!(t11_07_2064b3c_f7a3a57, "2064b3c..f7a3a57");
blame_test!(t11_08_0e17ccb_f7a3a57, "0e17ccb..f7a3a57");
blame_test!(t11_09_3be8265_f7a3a57, "3be8265..f7a3a57");
// blame_test!(t11_10_8bf8780_f7a3a57, "8bf8780..f7a3a57");

// 392db1b: Add a lot and blank lines
blame_test!(t12_392db1b, "392db1b");
blame_test!(t12_01_753d1db_392db1b, "753d1db..392db1b");
blame_test!(t12_02_f28f649_392db1b, "f28f649..392db1b");
blame_test!(t12_03_d3baed3_392db1b, "d3baed3..392db1b");
blame_test!(t12_04_536a0f5_392db1b, "536a0f5..392db1b");
blame_test!(t12_05_6a30c80_392db1b, "6a30c80..392db1b");
blame_test!(t12_06_4d8a3c7_392db1b, "4d8a3c7..392db1b");
blame_test!(t12_07_2064b3c_392db1b, "2064b3c..392db1b");
blame_test!(t12_08_0e17ccb_392db1b, "0e17ccb..392db1b");
blame_test!(t12_09_3be8265_392db1b, "3be8265..392db1b");
// blame_test!(t12_10_8bf8780_392db1b, "8bf8780..392db1b");
// blame_test!(t12_11_f7a3a57_392db1b, "f7a3a57..392db1b");

// bb48275: Side project
blame_test!(t13_bb48275, "bb48275");
blame_test!(t13_01_753d1db_bb48275, "753d1db..bb48275");
blame_test!(t13_02_f28f649_bb48275, "f28f649..bb48275");
blame_test!(t13_03_d3baed3_bb48275, "d3baed3..bb48275");
blame_test!(t13_04_536a0f5_bb48275, "536a0f5..bb48275");
blame_test!(t13_05_6a30c80_bb48275, "6a30c80..bb48275");
blame_test!(t13_06_4d8a3c7_bb48275, "4d8a3c7..bb48275");
blame_test!(t13_07_2064b3c_bb48275, "2064b3c..bb48275");
blame_test!(t13_08_0e17ccb_bb48275, "0e17ccb..bb48275");
blame_test!(t13_09_3be8265_bb48275, "3be8265..bb48275");
// blame_test!(t13_10_8bf8780_bb48275, "8bf8780..bb48275");
// blame_test!(t13_11_f7a3a57_bb48275, "f7a3a57..bb48275");
// blame_test!(t13_12_392db1b_bb48275, "392db1b..bb48275");

// c57fe89: Merge branch 'kek' into HEAD
blame_test!(t14_c57fe89, "c57fe89");
blame_test!(t14_01_753d1db_c57fe89, "753d1db..c57fe89");
blame_test!(t14_02_f28f649_c57fe89, "f28f649..c57fe89");
blame_test!(t14_03_d3baed3_c57fe89, "d3baed3..c57fe89");
blame_test!(t14_04_536a0f5_c57fe89, "536a0f5..c57fe89");
blame_test!(t14_05_6a30c80_c57fe89, "6a30c80..c57fe89");
blame_test!(t14_06_4d8a3c7_c57fe89, "4d8a3c7..c57fe89");
blame_test!(t14_07_2064b3c_c57fe89, "2064b3c..c57fe89");
blame_test!(t14_08_0e17ccb_c57fe89, "0e17ccb..c57fe89");
blame_test!(t14_09_3be8265_c57fe89, "3be8265..c57fe89");
// blame_test!(t14_10_8bf8780_c57fe89, "8bf8780..c57fe89");
// blame_test!(t14_11_f7a3a57_c57fe89, "f7a3a57..c57fe89");
// blame_test!(t14_12_392db1b_c57fe89, "392db1b..c57fe89");
// blame_test!(t14_13_bb48275_c57fe89, "bb48275..c57fe89");

// d7d6328: Multiple changes in one commit again
blame_test!(t15_d7d6328, "d7d6328");
blame_test!(t15_01_753d1db_d7d6328, "753d1db..d7d6328");
blame_test!(t15_02_f28f649_d7d6328, "f28f649..d7d6328");
blame_test!(t15_03_d3baed3_d7d6328, "d3baed3..d7d6328");
blame_test!(t15_04_536a0f5_d7d6328, "536a0f5..d7d6328");
blame_test!(t15_05_6a30c80_d7d6328, "6a30c80..d7d6328");
blame_test!(t15_06_4d8a3c7_d7d6328, "4d8a3c7..d7d6328");
blame_test!(t15_07_2064b3c_d7d6328, "2064b3c..d7d6328");
blame_test!(t15_08_0e17ccb_d7d6328, "0e17ccb..d7d6328");
blame_test!(t15_09_3be8265_d7d6328, "3be8265..d7d6328");
// blame_test!(t15_10_8bf8780_d7d6328, "8bf8780..d7d6328");
// blame_test!(t15_11_f7a3a57_d7d6328, "f7a3a57..d7d6328");
// blame_test!(t15_12_392db1b_d7d6328, "392db1b..d7d6328");
// blame_test!(t15_13_bb48275_d7d6328, "bb48275..d7d6328");
blame_test!(t15_14_c57fe89_d7d6328, "c57fe89..d7d6328");
