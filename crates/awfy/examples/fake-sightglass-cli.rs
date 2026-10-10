//! A stand-in for `sightglass-cli benchmark` in integration tests: writes
//! canned raw CSV for the one benchmark it is given, without Valgrind.
//!
//! Fails if the wasm path contains `$FAKE_SIGHTGLASS_FAIL`. Counts are
//! `$FAKE_SIGHTGLASS_BASE` (default 1000000) scaled per phase and event.

use std::path::PathBuf;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    assert_eq!(args[0], "benchmark", "unexpected subcommand");
    let value = |flag: &str| {
        let i = args
            .iter()
            .position(|a| a == flag)
            .unwrap_or_else(|| panic!("missing {flag}"));
        args[i + 1].clone()
    };
    let output = PathBuf::from(value("--output-file"));
    let name = value("--name");
    let flags = args
        .iter()
        .find_map(|a| a.strip_prefix("--engine-flags-common="))
        .expect("missing --engine-flags-common");
    assert_eq!(value("--processes"), "1");
    assert_eq!(value("--measure"), "callgrind");
    let wasm = args.last().unwrap();

    let fail = std::env::var("FAKE_SIGHTGLASS_FAIL").unwrap_or_default();
    if !fail.is_empty() && wasm.contains(&fail) {
        eprintln!("Error: benchmark {wasm} failed on purpose");
        std::process::exit(1);
    }

    let base: u64 = std::env::var("FAKE_SIGHTGLASS_BASE")
        .ok()
        .map(|b| b.parse().unwrap())
        .unwrap_or(1_000_000);
    let pid = std::process::id();
    let mut csv =
        String::from("arch,engine,engine_flags,wasm,process,iteration,phase,event,count\n");
    for (phase, scale) in [("Compilation", 1), ("Instantiation", 0), ("Execution", 2)] {
        for (event, fraction) in [("virtual-cycles", 1.0), ("instructions-retired", 0.5)] {
            let count = ((base << scale) as f64 * fraction) as u64;
            csv += &format!("x86_64,{name},{flags},{wasm},{pid},0,{phase},{event},{count}\n");
        }
    }
    std::fs::write(output, csv).unwrap();
}
