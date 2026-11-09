//! End-to-end tests of the fwx-cli binary. CI runs these on Linux, macOS and Windows.
//! Fixtures come from tools/make_fixtures.py (run before `cargo test`).

use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn cli(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_fwx-cli")).args(args).output().expect("run fwx-cli")
}

fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests").join("fixtures")
}

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("fwx-cli-test-{}-{name}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}

fn sha256(path: &Path) -> String {
    Sha256::digest(fs::read(path).unwrap()).iter().map(|b| format!("{b:02x}")).collect()
}

fn text(out: &[u8]) -> String {
    String::from_utf8_lossy(out).into_owned()
}

#[test]
fn version_and_help_succeed() {
    let v = cli(&["--version"]);
    assert_eq!(v.status.code(), Some(0));
    assert!(text(&v.stdout).contains(env!("CARGO_PKG_VERSION")));

    let h = cli(&["--help"]);
    assert_eq!(h.status.code(), Some(0));
    assert!(text(&h.stdout).contains("USAGE"));
}

#[test]
fn usage_errors_exit_2() {
    assert_eq!(cli(&[]).status.code(), Some(2));
    assert_eq!(cli(&["frobnicate"]).status.code(), Some(2));
    assert_eq!(cli(&["list"]).status.code(), Some(2));
    assert_eq!(cli(&["extract", "--bogus", "x.zip"]).status.code(), Some(2));
    assert_eq!(cli(&["extract", "--out"]).status.code(), Some(2));
}

#[test]
fn missing_file_exits_1_and_names_the_file() {
    let out = cli(&["list", "no-such-firmware.zip"]);
    assert_eq!(out.status.code(), Some(1));
    assert!(text(&out.stderr).contains("no-such-firmware.zip"));
}

#[test]
fn list_shows_images_in_zip() {
    let zip = fixtures().join("SAMSUNG_TEST.zip");
    let out = cli(&["list", zip.to_str().unwrap()]);
    assert_eq!(out.status.code(), Some(0));
    let s = text(&out.stdout);
    assert!(s.contains("system.img.lz4"), "{s}");
    assert!(s.contains("loose.img.lz4"), "{s}");
}

#[test]
fn extract_writes_every_image_with_correct_bytes() {
    let manifest = fs::read_to_string(fixtures().join("manifest.tsv")).unwrap();
    let mut expected: HashMap<(String, String), String> = HashMap::new();
    for line in manifest.lines() {
        let cols: Vec<&str> = line.split('\t').collect();
        expected.insert((cols[0].to_owned(), cols[1].to_owned()), cols[2].to_owned());
    }

    let out_root = scratch("extract");
    let zip = fixtures().join("SAMSUNG_TEST.zip");
    let out = cli(&["extract", "-o", out_root.to_str().unwrap(), zip.to_str().unwrap()]);
    assert_eq!(out.status.code(), Some(0), "{}", text(&out.stderr));

    let dir = out_root.join("SAMSUNG_TEST_img");
    let wanted: Vec<_> = expected.iter().filter(|((fw, _), _)| fw == "SAMSUNG_TEST.zip").collect();
    assert_eq!(wanted.len(), 9);
    for ((_, name), want) in wanted {
        assert_eq!(&sha256(&dir.join(name)), want, "hash mismatch for {name}");
    }
    assert!(!dir.join(".work").exists(), "work folder should be cleaned up");
    let _ = fs::remove_dir_all(&out_root);
}

#[test]
fn match_filter_limits_extraction() {
    let out_root = scratch("match");
    let zip = fixtures().join("SAMSUNG_TEST.zip");
    let out = cli(&["extract", "-m", "BL.IMG", "-o", out_root.to_str().unwrap(), zip.to_str().unwrap()]);
    assert_eq!(out.status.code(), Some(0), "{}", text(&out.stderr));

    let written: Vec<String> = fs::read_dir(out_root.join("SAMSUNG_TEST_img"))
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    assert_eq!(written.len(), 1, "{written:?}");
    assert_eq!(written[0], "bl.img");
    let _ = fs::remove_dir_all(&out_root);
}

#[test]
fn one_bad_file_does_not_stop_the_others() {
    let out_root = scratch("partial");
    let good = fixtures().join("loose.img.lz4");
    let out = cli(&[
        "extract",
        "-o",
        out_root.to_str().unwrap(),
        "no-such-firmware.zip",
        good.to_str().unwrap(),
    ]);
    assert_eq!(out.status.code(), Some(1));
    assert!(out_root.join("loose_img").join("loose.img").exists());
    let _ = fs::remove_dir_all(&out_root);
}
