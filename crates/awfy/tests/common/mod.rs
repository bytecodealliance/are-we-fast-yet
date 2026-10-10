//! Helpers shared by integration tests.

#![allow(dead_code)]

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

pub fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
}

/// A temp repo root holding a copy of the fixture `config.toml` and
/// `benchmark-data/`.
pub fn fixture_root() -> tempfile::TempDir {
    let tmp = tempfile::tempdir().unwrap();
    copy_dir(
        &fixtures().join("benchmark-data"),
        &tmp.path().join("benchmark-data"),
    );
    std::fs::copy(
        fixtures().join("config.toml"),
        tmp.path().join("config.toml"),
    )
    .unwrap();
    tmp
}

pub fn copy_dir(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).unwrap();
    for entry in std::fs::read_dir(from).unwrap() {
        let entry = entry.unwrap();
        let to = to.join(entry.file_name());
        if entry.file_type().unwrap().is_dir() {
            copy_dir(&entry.path(), &to);
        } else {
            std::fs::copy(entry.path(), to).unwrap();
        }
    }
}

/// Run the `awfy` binary with `--root <root>`.
pub fn awfy(root: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_awfy"))
        .arg("--root")
        .arg(root)
        .args(args)
        .output()
        .unwrap()
}

/// Run `awfy`, assert success, and return stdout.
#[track_caller]
pub fn awfy_ok(root: &Path, args: &[&str]) -> String {
    let out = awfy(root, args);
    assert!(
        out.status.success(),
        "awfy {args:?} failed: {}\n{}",
        out.status,
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8(out.stdout).unwrap()
}
