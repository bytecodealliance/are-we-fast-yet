//! Benchmark slugs, for site file names and URLs. `site/js/slug.js` must
//! match; `site/tests/slug-cases.json` holds cases both test.

/// `bz2/benchmark.wasm` becomes `bz2-benchmark`: drop `.wasm`, turn `/` into
/// `-`, and any other character outside `[A-Za-z0-9._-]` into `_`.
///
/// Slugs are not reversible. The site maps them back by looking them up in
/// `index.json`'s benchmark list, and `awfy site` rejects collisions.
pub fn slug(benchmark: &str) -> String {
    let name = benchmark.strip_suffix(".wasm").unwrap_or(benchmark);
    name.chars()
        .map(|c| match c {
            'A'..='Z' | 'a'..='z' | '0'..='9' | '.' | '_' | '-' => c,
            '/' => '-',
            _ => '_',
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cases() {
        for (benchmark, want) in [
            ("bz2/benchmark.wasm", "bz2-benchmark"),
            ("shootout/shootout-sieve.wasm", "shootout-shootout-sieve"),
            ("Geomean", "Geomean"),
            ("a b/c+d.wasm", "a_b-c_d"),
            ("x.wasm.wasm", "x.wasm"),
            ("caf\u{e9}/\u{1f600}.wasm", "caf_-_"),
        ] {
            assert_eq!(slug(benchmark), want, "{benchmark}");
        }
    }

    /// Every benchmark in Sightglass gets its own slug, made only of
    /// characters that need no escaping in a URL or a file name.
    #[test]
    fn real_benchmarks_round_trip() {
        let benchmarks =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../sightglass/benchmarks");
        let mut wasms =
            crate::run::parse_suite(&benchmarks.join("all.suite"), &benchmarks).unwrap();
        wasms.push("Geomean".into());
        let mut seen = std::collections::BTreeMap::new();
        for wasm in &wasms {
            let wasm = wasm.to_str().unwrap();
            let s = slug(wasm);
            assert!(
                s.bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b))
            );
            if let Some(other) = seen.insert(s.clone(), wasm) {
                assert_eq!(other, wasm, "slug {s} collides");
            }
        }
        assert!(seen.len() > 20);
    }
}
