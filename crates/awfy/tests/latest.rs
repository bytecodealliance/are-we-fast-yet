mod common;

use common::{awfy, awfy_ok, fixture_root};

#[test]
fn latest_visible_run() {
    let root = fixture_root();
    assert_eq!(
        awfy_ok(root.path(), &["latest", "wasmtime.commit"]),
        "6666666666666666666666666666666666666666\n"
    );
    assert_eq!(
        awfy_ok(root.path(), &["latest", "started"]),
        "2026-10-06T03:17:00Z\n"
    );
}

#[test]
fn latest_skips_excluded_runs() {
    let root = fixture_root();
    let data = root.path().join("benchmark-data");
    std::fs::remove_dir_all(data.join("00005")).unwrap();
    std::fs::remove_dir_all(data.join("00006")).unwrap();
    // 00004 is excluded.
    assert_eq!(
        awfy_ok(root.path(), &["latest", "wasmtime.commit"]),
        "3333333333333333333333333333333333333333\n"
    );
}

#[test]
fn latest_on_empty_repo() {
    let root = tempfile::tempdir().unwrap();
    assert_eq!(awfy_ok(root.path(), &["latest", "wasmtime.commit"]), "");
    std::fs::create_dir(root.path().join("benchmark-data")).unwrap();
    assert_eq!(awfy_ok(root.path(), &["latest", "wasmtime.commit"]), "");
}

#[test]
fn latest_unknown_field() {
    let root = fixture_root();
    assert!(
        !awfy(root.path(), &["latest", "wasmtime.nope"])
            .status
            .success()
    );
    let empty = tempfile::tempdir().unwrap();
    assert!(!awfy(empty.path(), &["latest", "nope"]).status.success());
}
