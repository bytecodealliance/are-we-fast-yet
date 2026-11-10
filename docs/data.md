# Data

`benchmark-data/NNNNN/` holds one run: `sightglass.csv.gz` and `metadata.toml`.

* Run directories are written once and never edited. Fix bad data with a new
  run or an entry in `benchmark-data/excluded.toml`, which hides a run from the
  site and from regression baselines.
* A run with some failed benchmarks is still committed. Its failures are listed
  in metadata.
* The CSV is Sightglass's raw output with `engine` and `wasm` normalized so it
  is stable and reproducible across runs. Within a (benchmark, phase, event),
  rows are in iteration order, whichever shard measured them. It holds only
  measured samples: medians, geomeans, and noise are all derived from them at
  deploy time, so a change to how they are computed needs no rewrite of recorded
  data.
* `[wasmtime]` records the Wasmtime commits new since the previous run, for
  the run page's commit list. They are captured at record time, since the
  site is built by a job with no Wasmtime clone to read them from. A long log
  is truncated, with the rest counted in `omitted-commits`.

## Hosts

A run is measured in shards on different runners, so `[[environment.hosts]]`
records each shard's host: the iterations it measured, `uname`, the CPU model,
and the CPU's feature flags. That makes it possible to check whether hosts
measure differently, whether Valgrind passing CPU feature bits through steers
the code that runs, and, if host effects ever matter, to switch to a test
stratified by host.

Metadata schema 1 recorded a single `uname` and `cpu` for the whole run.
`awfy` still reads it, as one host covering every iteration with no flags.

## Environment fingerprint

The fingerprint covers what we change deliberately: the Sightglass commit,
Valgrind, rustc, and `[config]`. Hosts are recorded but excluded, because
runners vary. A fingerprint change gets a marker
on the site and skips regression detection for that pair of runs.
