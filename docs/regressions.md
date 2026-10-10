# Regression detection

`awfy regressions --run NNNNN` compares a run with its baseline and prints an
issue (title and Markdown body) or nothing. It runs from `benchmark.yml` right
after a new run is pushed. It reads only committed data and never re-runs
benchmarks.

## Baseline

The baseline is the newest earlier run that is not in `excluded.toml`. No
comparison happens, and the job summary says why, when:

* the run itself is excluded,
* there is no baseline,
* the environment fingerprints differ (see [data.md](data.md)), or
* the Wasmtime commits are equal (e.g. a forced re-run).

Benchmarks missing from either run are skipped and listed in the job summary.

## Test

1. Load both CSVs. Rename `engine` to the run number so Sightglass sees two
   engines.
2. Keep only `event = virtual-cycles`. Add Sightglass's "Geomean"
   pseudo-benchmark (`sightglass_analysis::geomean::add`), which aggregates
   only benchmarks present in both runs.
3. Call `sightglass_analysis::effect_size::calculate` at the configured
   significance level.
4. Report a (benchmark, phase) as regressed when **both**:
   * the result is significant, and
   * the new mean exceeds the old by at least `min-relative-change`.

The second condition exists because callgrind is near-deterministic: variance
is about zero, so trivial changes are "significant". The threshold is the real
filter.

Speedups use the same test and are listed in the job summary but never file
issues.

```toml
# config.toml
[regressions]
significance-level = 0.01
min-relative-change = 0.01   # 1%; calibrate during rollout
issue-repo = "bytecodealliance/wasmtime"
```

## Issue

Filed on `regressions.issue-repo` with the App token (see
[tokens.md](tokens.md)).

Title: `Performance regression in <short-old>..<short-new> (are-we-fast-yet run NNNNN)`

Body:

* Summary: run numbers, links to both runs' pages and metadata.
* Table, sorted by effect size, Geomean rows first:

  | benchmark | phase | effect size |
  |-----------|-------|-------------|
  | `bz2/benchmark.wasm` | compilation | 1.23x +/- 0.004 more virtual cycles |

  Each benchmark name links to its trend graph on the site.
* Collapsed `<details>`: other callgrind counters that changed significantly
  for the regressed (benchmark, phase) pairs, to hint at the cause (e.g. cache
  misses vs. instructions).
* Commit range: compare link plus `git log --oneline old..new`, truncated at
  50 commits.
* How to reproduce locally: the exact commands to build both engines and run
  `sightglass-cli benchmark --measure callgrind` on the regressed benchmarks,
  including the pinned `--engine-flags-common`.

### Commit log

`awfy regressions --wasmtime-repo <path>` shells out to `git log`. The workflow
provides a treeless clone (`git clone --filter=tree:0 --no-checkout`), which is
fast and needs no token.

### Idempotency

Before creating an issue, the workflow searches `issue-repo` for an issue by the
App with `(are-we-fast-yet run NNNNN)` in the title. If one exists, it does
nothing. Re-running a failed workflow therefore never files duplicates.

### No deduplication across days

A regression is reported once, for the day it lands. Later runs compare against
the already-regressed baseline and stay quiet. A fix shows up as a speedup in
the job summary.

## Rollout

1. **Dry runs.** `workflow_dispatch` with `dry-run`; inspect the would-be issue
   artifact.
2. **Observation.** Daily cron on, `issue-repo` set to this repo. Collect a few
   weeks of data, then set `min-relative-change` from observed run-to-run noise
   on unchanged code paths.
3. **Live.** Switch `issue-repo` to `bytecodealliance/wasmtime`.
