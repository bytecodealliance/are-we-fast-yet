mod common;

use common::{awfy_ok, fixture_root};
use std::path::Path;

fn tree(dir: &Path) -> Vec<String> {
    let mut out = Vec::new();
    for entry in walk(dir) {
        out.push(
            entry
                .strip_prefix(dir)
                .unwrap()
                .to_str()
                .unwrap()
                .replace('\\', "/"),
        );
    }
    out.sort();
    out
}

fn walk(dir: &Path) -> Vec<std::path::PathBuf> {
    let mut out = Vec::new();
    for entry in std::fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            out.extend(walk(&path));
        } else {
            out.push(path);
        }
    }
    out
}

#[test]
fn site_writes_static_files_and_json() {
    let root = fixture_root();
    let site = root.path().join("site");
    std::fs::create_dir_all(site.join("js")).unwrap();
    std::fs::create_dir_all(site.join("tests")).unwrap();
    std::fs::create_dir_all(site.join("node_modules/x")).unwrap();
    std::fs::write(site.join("index.html"), "<!doctype html>").unwrap();
    std::fs::write(site.join("js/app.js"), "").unwrap();
    std::fs::write(site.join("package.json"), "{}").unwrap();
    std::fs::write(site.join("tests/t.js"), "").unwrap();
    std::fs::write(site.join("node_modules/x/y.js"), "").unwrap();

    let out = root.path().join("_site");
    // Stale output is removed.
    std::fs::create_dir_all(out.join("data")).unwrap();
    std::fs::write(out.join("stale.html"), "").unwrap();

    awfy_ok(
        root.path(),
        &[
            "site",
            "--out",
            out.to_str().unwrap(),
            "--now",
            "2026-10-10T04:30:00Z",
        ],
    );
    assert_eq!(
        tree(&out),
        [
            "data/benchmarks/Geomean.json",
            "data/benchmarks/bz2-benchmark.json",
            "data/benchmarks/pulldown-cmark-benchmark.json",
            "data/index.json",
            "data/runs/00001.json",
            "data/runs/00002.json",
            "data/runs/00003.json",
            "data/runs/00005.json",
            "data/runs/00006.json",
            "index.html",
            "js/app.js",
        ]
    );
    let index = std::fs::read_to_string(out.join("data/index.json")).unwrap();
    assert!(
        index.starts_with(r#"{"schema":1,"generated":"2026-10-10T04:30:00Z","repo":"bytecodealliance/are-we-fast-yet","runs":[{"run":1,"#)
    );
}
