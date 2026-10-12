# Data

`benchmark-data/NNNNN/` holds one run: `sightglass.csv.gz` and `metadata.toml`.

* Run directories are written once and never edited. Fix bad data with a new
  run or an entry in `benchmark-data/excluded.toml`, which hides a run from the
  site and from regression baselines.
* A run with some failed benchmarks is still committed. Its failures are listed
  in metadata.
* The CSV is Sightglass's raw output with `engine`, `wasm`, and `process`
  normalized so it is stable and reproducible across runs.

## Environment fingerprint

The fingerprint covers what we change deliberately: the Sightglass commit,
Valgrind, rustc, and `[config]`. Host details such as `uname` and `cpu` are
recorded but excluded, because runners vary. A fingerprint change gets a marker
on the site and skips regression detection for that pair of runs.
