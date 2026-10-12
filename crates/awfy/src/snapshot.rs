//! Snapshot testing: compare text with a checked-in file under
//! `crates/awfy/tests/snapshots/`. Run with `AWFY_UPDATE_SNAPSHOTS=1` to
//! write the files instead, then review the diff.

use std::path::PathBuf;

fn path(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/snapshots")
        .join(name)
}

#[track_caller]
pub fn assert_snapshot(name: &str, actual: &str) {
    let path = path(name);
    if std::env::var_os("AWFY_UPDATE_SNAPSHOTS").is_some() {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, actual).unwrap();
        return;
    }
    let expected = std::fs::read_to_string(&path).unwrap_or_else(|e| {
        panic!(
            "reading snapshot {}: {e}; run with AWFY_UPDATE_SNAPSHOTS=1 to create it",
            path.display()
        )
    });
    if expected != actual {
        panic!(
            "snapshot {} differs; run with AWFY_UPDATE_SNAPSHOTS=1 to update it\n\
             --- expected\n{expected}\n--- actual\n{actual}",
            path.display()
        );
    }
}

/// Pretty JSON with a trailing newline.
pub fn json(value: &impl serde::Serialize) -> String {
    serde_json::to_string_pretty(value).unwrap() + "\n"
}
