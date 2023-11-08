// 753d1db: Initial commit
topo_test!(t01_753d1db, "753d1db");

// f28f649: Simple change
topo_test!(t02_f28f649, "f28f649");
topo_test!(t02_01_753d1db_f28f649, "753d1db..f28f649");

// d3baed3: Removes more than it adds
topo_test!(t03_d3baed3, "d3baed3");
topo_test!(t03_01_753d1db_d3baed3, "753d1db..d3baed3");
topo_test!(t03_02_f28f649_d3baed3, "f28f649..d3baed3");

// 536a0f5: Adds more than it removes
topo_test!(t04_536a0f5, "536a0f5");
topo_test!(t04_01_753d1db_536a0f5, "753d1db..536a0f5");
topo_test!(t04_02_f28f649_536a0f5, "f28f649..536a0f5");
topo_test!(t04_03_d3baed3_536a0f5, "d3baed3..536a0f5");

// 6a30c80: Change on first line
topo_test!(t05_6a30c80, "6a30c80");
topo_test!(t05_01_753d1db_6a30c80, "753d1db..6a30c80");
topo_test!(t05_02_f28f649_6a30c80, "f28f649..6a30c80");
topo_test!(t05_03_d3baed3_6a30c80, "d3baed3..6a30c80");
topo_test!(t05_04_536a0f5_6a30c80, "536a0f5..6a30c80");

// 4d8a3c7: Multiple changes in one commit
topo_test!(t06_4d8a3c7, "4d8a3c7");
topo_test!(t06_01_753d1db_4d8a3c7, "753d1db..4d8a3c7");
topo_test!(t06_02_f28f649_4d8a3c7, "f28f649..4d8a3c7");
topo_test!(t06_03_d3baed3_4d8a3c7, "d3baed3..4d8a3c7");
topo_test!(t06_04_536a0f5_4d8a3c7, "536a0f5..4d8a3c7");
topo_test!(t06_05_6a30c80_4d8a3c7, "6a30c80..4d8a3c7");

// 2064b3c: Change on last line
topo_test!(t07_2064b3c, "2064b3c");
topo_test!(t07_01_753d1db_2064b3c, "753d1db..2064b3c");
topo_test!(t07_02_f28f649_2064b3c, "f28f649..2064b3c");
topo_test!(t07_03_d3baed3_2064b3c, "d3baed3..2064b3c");
topo_test!(t07_04_536a0f5_2064b3c, "536a0f5..2064b3c");
topo_test!(t07_05_6a30c80_2064b3c, "6a30c80..2064b3c");
topo_test!(t07_06_4d8a3c7_2064b3c, "4d8a3c7..2064b3c");

// 0e17ccb: Blank line in context
topo_test!(t08_0e17ccb, "0e17ccb");
topo_test!(t08_01_753d1db_0e17ccb, "753d1db..0e17ccb");
topo_test!(t08_02_f28f649_0e17ccb, "f28f649..0e17ccb");
topo_test!(t08_03_d3baed3_0e17ccb, "d3baed3..0e17ccb");
topo_test!(t08_04_536a0f5_0e17ccb, "536a0f5..0e17ccb");
topo_test!(t08_05_6a30c80_0e17ccb, "6a30c80..0e17ccb");
topo_test!(t08_06_4d8a3c7_0e17ccb, "4d8a3c7..0e17ccb");
topo_test!(t08_07_2064b3c_0e17ccb, "2064b3c..0e17ccb");

// 3be8265: Indent and overlap with previous change.
topo_test!(t09_3be8265, "3be8265");
topo_test!(t09_01_753d1db_3be8265, "753d1db..3be8265");
topo_test!(t09_02_f28f649_3be8265, "f28f649..3be8265");
topo_test!(t09_03_d3baed3_3be8265, "d3baed3..3be8265");
topo_test!(t09_04_536a0f5_3be8265, "536a0f5..3be8265");
topo_test!(t09_05_6a30c80_3be8265, "6a30c80..3be8265");
topo_test!(t09_06_4d8a3c7_3be8265, "4d8a3c7..3be8265");
topo_test!(t09_07_2064b3c_3be8265, "2064b3c..3be8265");
topo_test!(t09_08_0e17ccb_3be8265, "0e17ccb..3be8265");

// 8bf8780: Simple change but a bit bigger
topo_test!(t10_8bf8780, "8bf8780");
topo_test!(t10_01_753d1db_8bf8780, "753d1db..8bf8780");
topo_test!(t10_02_f28f649_8bf8780, "f28f649..8bf8780");
topo_test!(t10_03_d3baed3_8bf8780, "d3baed3..8bf8780");
topo_test!(t10_04_536a0f5_8bf8780, "536a0f5..8bf8780");
topo_test!(t10_05_6a30c80_8bf8780, "6a30c80..8bf8780");
topo_test!(t10_06_4d8a3c7_8bf8780, "4d8a3c7..8bf8780");
topo_test!(t10_07_2064b3c_8bf8780, "2064b3c..8bf8780");
topo_test!(t10_08_0e17ccb_8bf8780, "0e17ccb..8bf8780");
topo_test!(t10_09_3be8265_8bf8780, "3be8265..8bf8780");

// f7a3a57: Remove a lot
topo_test!(t11_f7a3a57, "f7a3a57");
topo_test!(t11_01_753d1db_f7a3a57, "753d1db..f7a3a57");
topo_test!(t11_02_f28f649_f7a3a57, "f28f649..f7a3a57");
topo_test!(t11_03_d3baed3_f7a3a57, "d3baed3..f7a3a57");
topo_test!(t11_04_536a0f5_f7a3a57, "536a0f5..f7a3a57");
topo_test!(t11_05_6a30c80_f7a3a57, "6a30c80..f7a3a57");
topo_test!(t11_06_4d8a3c7_f7a3a57, "4d8a3c7..f7a3a57");
topo_test!(t11_07_2064b3c_f7a3a57, "2064b3c..f7a3a57");
topo_test!(t11_08_0e17ccb_f7a3a57, "0e17ccb..f7a3a57");
topo_test!(t11_09_3be8265_f7a3a57, "3be8265..f7a3a57");
topo_test!(t11_10_8bf8780_f7a3a57, "8bf8780..f7a3a57");

// 392db1b: Add a lot and blank lines
topo_test!(t12_392db1b, "392db1b");
topo_test!(t12_01_753d1db_392db1b, "753d1db..392db1b");
topo_test!(t12_02_f28f649_392db1b, "f28f649..392db1b");
topo_test!(t12_03_d3baed3_392db1b, "d3baed3..392db1b");
topo_test!(t12_04_536a0f5_392db1b, "536a0f5..392db1b");
topo_test!(t12_05_6a30c80_392db1b, "6a30c80..392db1b");
topo_test!(t12_06_4d8a3c7_392db1b, "4d8a3c7..392db1b");
topo_test!(t12_07_2064b3c_392db1b, "2064b3c..392db1b");
topo_test!(t12_08_0e17ccb_392db1b, "0e17ccb..392db1b");
topo_test!(t12_09_3be8265_392db1b, "3be8265..392db1b");
topo_test!(t12_10_8bf8780_392db1b, "8bf8780..392db1b");
topo_test!(t12_11_f7a3a57_392db1b, "f7a3a57..392db1b");

// 616867d: October
topo_test!(t13_616867d, "616867d");
topo_test!(t13_01_753d1db_616867d, "753d1db..616867d");
topo_test!(t13_02_f28f649_616867d, "f28f649..616867d");
topo_test!(t13_03_d3baed3_616867d, "d3baed3..616867d");
topo_test!(t13_04_536a0f5_616867d, "536a0f5..616867d");
topo_test!(t13_05_6a30c80_616867d, "6a30c80..616867d");
topo_test!(t13_06_4d8a3c7_616867d, "4d8a3c7..616867d");
topo_test!(t13_07_2064b3c_616867d, "2064b3c..616867d");
topo_test!(t13_08_0e17ccb_616867d, "0e17ccb..616867d");
topo_test!(t13_09_3be8265_616867d, "3be8265..616867d");
topo_test!(t13_10_8bf8780_616867d, "8bf8780..616867d");
topo_test!(t13_11_f7a3a57_616867d, "f7a3a57..616867d");
topo_test!(t13_12_392db1b_616867d, "392db1b..616867d");

// bb48275: Side project
topo_test!(t14_bb48275, "bb48275");
topo_test!(t14_01_753d1db_bb48275, "753d1db..bb48275");
topo_test!(t14_02_f28f649_bb48275, "f28f649..bb48275");
topo_test!(t14_03_d3baed3_bb48275, "d3baed3..bb48275");
topo_test!(t14_04_536a0f5_bb48275, "536a0f5..bb48275");
topo_test!(t14_05_6a30c80_bb48275, "6a30c80..bb48275");
topo_test!(t14_06_4d8a3c7_bb48275, "4d8a3c7..bb48275");
topo_test!(t14_07_2064b3c_bb48275, "2064b3c..bb48275");
topo_test!(t14_08_0e17ccb_bb48275, "0e17ccb..bb48275");
topo_test!(t14_09_3be8265_bb48275, "3be8265..bb48275");
topo_test!(t14_10_8bf8780_bb48275, "8bf8780..bb48275");
topo_test!(t14_11_f7a3a57_bb48275, "f7a3a57..bb48275");
topo_test!(t14_12_392db1b_bb48275, "392db1b..bb48275");
topo_test!(t14_13_616867d_bb48275, "616867d..bb48275");

// bb8601c: Merge branch 'kek2' into HEAD
topo_test!(t15_bb8601c, "bb8601c");
topo_test!(t15_01_753d1db_bb8601c, "753d1db..bb8601c");
topo_test!(t15_02_f28f649_bb8601c, "f28f649..bb8601c");
topo_test!(t15_03_d3baed3_bb8601c, "d3baed3..bb8601c");
topo_test!(t15_04_536a0f5_bb8601c, "536a0f5..bb8601c");
topo_test!(t15_05_6a30c80_bb8601c, "6a30c80..bb8601c");
topo_test!(t15_06_4d8a3c7_bb8601c, "4d8a3c7..bb8601c");
topo_test!(t15_07_2064b3c_bb8601c, "2064b3c..bb8601c");
topo_test!(t15_08_0e17ccb_bb8601c, "0e17ccb..bb8601c");
topo_test!(t15_09_3be8265_bb8601c, "3be8265..bb8601c");
topo_test!(t15_10_8bf8780_bb8601c, "8bf8780..bb8601c");
topo_test!(t15_11_f7a3a57_bb8601c, "f7a3a57..bb8601c");
topo_test!(t15_12_392db1b_bb8601c, "392db1b..bb8601c");
topo_test!(t15_13_616867d_bb8601c, "616867d..bb8601c");
topo_test!(t15_14_bb48275_bb8601c, "bb48275..bb8601c");

// 00491e2: Multiple changes in one commit again
topo_test!(t16_00491e2, "00491e2");
topo_test!(t16_01_753d1db_00491e2, "753d1db..00491e2");
topo_test!(t16_02_f28f649_00491e2, "f28f649..00491e2");
topo_test!(t16_03_d3baed3_00491e2, "d3baed3..00491e2");
topo_test!(t16_04_536a0f5_00491e2, "536a0f5..00491e2");
topo_test!(t16_05_6a30c80_00491e2, "6a30c80..00491e2");
topo_test!(t16_06_4d8a3c7_00491e2, "4d8a3c7..00491e2");
topo_test!(t16_07_2064b3c_00491e2, "2064b3c..00491e2");
topo_test!(t16_08_0e17ccb_00491e2, "0e17ccb..00491e2");
topo_test!(t16_09_3be8265_00491e2, "3be8265..00491e2");
topo_test!(t16_10_8bf8780_00491e2, "8bf8780..00491e2");
topo_test!(t16_11_f7a3a57_00491e2, "f7a3a57..00491e2");
topo_test!(t16_12_392db1b_00491e2, "392db1b..00491e2");
topo_test!(t16_13_616867d_00491e2, "616867d..00491e2");
topo_test!(t16_14_bb48275_00491e2, "bb48275..00491e2");
topo_test!(t16_15_bb8601c_00491e2, "bb8601c..00491e2");

// d87231e: More changes
topo_test!(t17_d87231e, "d87231e");
topo_test!(t17_01_753d1db_d87231e, "753d1db..d87231e");
topo_test!(t17_02_f28f649_d87231e, "f28f649..d87231e");
topo_test!(t17_03_d3baed3_d87231e, "d3baed3..d87231e");
topo_test!(t17_04_536a0f5_d87231e, "536a0f5..d87231e");
topo_test!(t17_05_6a30c80_d87231e, "6a30c80..d87231e");
topo_test!(t17_06_4d8a3c7_d87231e, "4d8a3c7..d87231e");
topo_test!(t17_07_2064b3c_d87231e, "2064b3c..d87231e");
topo_test!(t17_08_0e17ccb_d87231e, "0e17ccb..d87231e");
topo_test!(t17_09_3be8265_d87231e, "3be8265..d87231e");
topo_test!(t17_10_8bf8780_d87231e, "8bf8780..d87231e");
topo_test!(t17_11_f7a3a57_d87231e, "f7a3a57..d87231e");
topo_test!(t17_12_392db1b_d87231e, "392db1b..d87231e");
topo_test!(t17_13_616867d_d87231e, "616867d..d87231e");
topo_test!(t17_14_bb48275_d87231e, "bb48275..d87231e");
topo_test!(t17_15_bb8601c_d87231e, "bb8601c..d87231e");
topo_test!(t17_16_00491e2_d87231e, "00491e2..d87231e");

// 51c8d7c: Merge branch 'kek3' into third-test
topo_test!(t18_51c8d7c, "51c8d7c");
topo_test!(t18_01_753d1db_51c8d7c, "753d1db..51c8d7c");
topo_test!(t18_02_f28f649_51c8d7c, "f28f649..51c8d7c");
topo_test!(t18_03_d3baed3_51c8d7c, "d3baed3..51c8d7c");
topo_test!(t18_04_536a0f5_51c8d7c, "536a0f5..51c8d7c");
topo_test!(t18_05_6a30c80_51c8d7c, "6a30c80..51c8d7c");
topo_test!(t18_06_4d8a3c7_51c8d7c, "4d8a3c7..51c8d7c");
topo_test!(t18_07_2064b3c_51c8d7c, "2064b3c..51c8d7c");
topo_test!(t18_08_0e17ccb_51c8d7c, "0e17ccb..51c8d7c");
topo_test!(t18_09_3be8265_51c8d7c, "3be8265..51c8d7c");
topo_test!(t18_10_8bf8780_51c8d7c, "8bf8780..51c8d7c");
topo_test!(t18_11_f7a3a57_51c8d7c, "f7a3a57..51c8d7c");
topo_test!(t18_12_392db1b_51c8d7c, "392db1b..51c8d7c");
topo_test!(t18_13_616867d_51c8d7c, "616867d..51c8d7c");
topo_test!(t18_14_bb48275_51c8d7c, "bb48275..51c8d7c");
topo_test!(t18_15_bb8601c_51c8d7c, "bb8601c..51c8d7c");
topo_test!(t18_16_00491e2_51c8d7c, "00491e2..51c8d7c");
topo_test!(t18_17_d87231e_51c8d7c, "d87231e..51c8d7c");

// b282e76: More changes
topo_test!(t19_b282e76, "b282e76");
topo_test!(t19_01_753d1db_b282e76, "753d1db..b282e76");
topo_test!(t19_02_f28f649_b282e76, "f28f649..b282e76");
topo_test!(t19_03_d3baed3_b282e76, "d3baed3..b282e76");
topo_test!(t19_04_536a0f5_b282e76, "536a0f5..b282e76");
topo_test!(t19_05_6a30c80_b282e76, "6a30c80..b282e76");
topo_test!(t19_06_4d8a3c7_b282e76, "4d8a3c7..b282e76");
topo_test!(t19_07_2064b3c_b282e76, "2064b3c..b282e76");
topo_test!(t19_08_0e17ccb_b282e76, "0e17ccb..b282e76");
topo_test!(t19_09_3be8265_b282e76, "3be8265..b282e76");
topo_test!(t19_10_8bf8780_b282e76, "8bf8780..b282e76");
topo_test!(t19_11_f7a3a57_b282e76, "f7a3a57..b282e76");
topo_test!(t19_12_392db1b_b282e76, "392db1b..b282e76");
topo_test!(t19_13_616867d_b282e76, "616867d..b282e76");
topo_test!(t19_14_bb48275_b282e76, "bb48275..b282e76");
topo_test!(t19_15_bb8601c_b282e76, "bb8601c..b282e76");
topo_test!(t19_16_00491e2_b282e76, "00491e2..b282e76");
topo_test!(t19_17_d87231e_b282e76, "d87231e..b282e76");
topo_test!(t19_18_51c8d7c_b282e76, "51c8d7c..b282e76");
