//! Checks a run recorded with real Sightglass and Valgrind. `ci.yml`'s
//! `smoke` job records one and then runs:
//!
//! ```sh
//! AWFY_SMOKE_ROOT=<root> cargo test -p awfy --test smoke -- --ignored
//! ```

use awfy::runs::DataDir;
use sightglass_data::Phase;
use std::collections::BTreeSet;
use std::path::Path;

/// `EVENT_MAPPINGS` in `sightglass/crates/recorder/src/measure/callgrind.rs`,
/// plus `virtual-cycles`.
fn expected_events() -> BTreeSet<String> {
    let source = std::fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../sightglass/crates/recorder/src/measure/callgrind.rs"),
    )
    .unwrap();
    let start = source
        .find("const EVENT_MAPPINGS")
        .expect("no EVENT_MAPPINGS");
    let end = start + source[start..].find("];").unwrap();
    let mut events: BTreeSet<String> = source[start..end]
        .lines()
        .filter_map(|l| {
            // `("Ir", "instructions-retired"),`
            let quoted: Vec<&str> = l.split('"').collect();
            (quoted.len() >= 5).then(|| quoted[3].to_string())
        })
        .collect();
    assert!(events.len() > 5, "parsed {events:?}");
    events.insert("virtual-cycles".into());
    events
}

#[test]
#[ignore]
fn smoke() {
    let root = std::env::var("AWFY_SMOKE_ROOT").expect("set AWFY_SMOKE_ROOT");
    let data = DataDir::new(Path::new(&root));
    let run = *data.runs().unwrap().last().expect("no runs recorded");

    let metadata = data.metadata(run).unwrap();
    assert!(metadata.failures.is_empty(), "{:?}", metadata.failures);
    let toml: toml::Table = metadata.to_toml().unwrap().parse().unwrap();
    for (section, value) in &toml {
        match value {
            toml::Value::Table(t) if section != "failures" => {
                for (key, v) in t {
                    assert!(
                        v.as_str()
                            .is_none_or(|s| !s.trim().is_empty() && s != "unknown"),
                        "{section}.{key} is empty"
                    );
                }
            }
            _ => {}
        }
    }
    assert!(metadata.environment.valgrind.starts_with("valgrind-"));
    assert!(metadata.environment.rustc.starts_with("rustc "));

    let measurements = data.measurements(run).unwrap();
    let benchmarks: BTreeSet<&str> = measurements.iter().map(|m| m.wasm.as_ref()).collect();
    assert_eq!(benchmarks, ["noop/benchmark.wasm"].into());
    let want = expected_events();
    for phase in [Phase::Compilation, Phase::Instantiation, Phase::Execution] {
        let got: BTreeSet<String> = measurements
            .iter()
            .filter(|m| m.phase == phase)
            .map(|m| m.event.to_string())
            .collect();
        assert_eq!(got, want, "events for {phase}");
    }
    assert!(measurements.iter().all(|m| m.engine.name == "wasmtime"));
    let cycles = measurements
        .iter()
        .find(|m| m.event == "virtual-cycles" && m.phase == Phase::Compilation)
        .unwrap();
    assert!(cycles.count > 0);
}

/// The event list parses, so a Sightglass bump that moves it fails here.
#[test]
fn event_mappings_parse() {
    let events = expected_events();
    assert!(events.contains("instructions-retired"));
    assert!(events.contains("virtual-cycles"));
}
