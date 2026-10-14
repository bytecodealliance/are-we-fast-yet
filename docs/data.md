# Data

`benchmark-data/NNNNN/` holds one run: `sightglass.csv.gz` and `metadata.toml`.

* Run directories are written once and never edited. Fix bad data with a new
  run or an entry in `benchmark-data/excluded.toml`, which hides a run from the
  site and from regression baselines.
* A run with some failed benchmarks is still committed. Its failures are listed
  in metadata.
* The CSV is Sightglass's raw output with `engine` and `wasm` normalized so
  it is stable and reproducible across runs. It holds only measured samples:
  medians, geomeans, and noise are all derived from them at deploy time, so a
  change to how they are computed needs no rewrite of recorded data.
* `[wasmtime]` records the Wasmtime commits new since the previous run, for
  the run page's commit list. They are captured at record time, since the
  site is built by a job with no Wasmtime clone to read them from. A long log
  is truncated, with the rest counted in `omitted-commits`.

## Environment fingerprint

The fingerprint covers what we change deliberately: the Sightglass commit,
Valgrind, rustc, and `[config]`. Host details such as `uname` and `cpu` are
recorded but excluded, because runners vary. A fingerprint change gets a marker
on the site and skips regression detection for that pair of runs.
