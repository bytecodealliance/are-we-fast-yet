# Architecture

A GitHub Actions run every three hours benchmarks Wasmtime `main` under
Sightglass's callgrind measure, commits the raw results, files an issue on
`bytecodealliance/wasmtime` for significant virtual-cycles regressions, and
deploys trend plots to GitHub Pages. A run that finds Wasmtime `main`
unchanged since the last one exits early, so the schedule sets how soon a new
commit is measured, not how many runs are recorded.

Non-goals: wall-clock or `perf` measurements (runners are too noisy), dedicated
hardware or external services, benchmarking PRs, and backfilling history.

## Principles

* Everything lives in git. Actions is the only compute; Pages the only hosting.
* Workflows stay thin. Logic lives in `awfy`, so it runs and tests locally.
* Raw data is the source of truth. Derived data is regenerated at deploy time.
* The Wasmtime repo is untouched; filing issues needs only a GitHub App.

## Key decisions

* **One runner, many processes.** Callgrind counts don't depend on load, so
  `awfy run` runs `nproc` units at once on one host, each unit one iteration of
  one benchmark in its own process. If a run outgrows the 6 hour job limit,
  shard across a matrix.
* **Pinned environment.** `.github/actions/install-valgrind` pins Valgrind,
  `rust-toolchain.toml` pins rustc, and the `sightglass/` submodule pins the
  suite and cost model. Changing any of them changes the fingerprint (see
  [data.md](data.md)).
* **Pinned Cranelift ISA flags.** Runner CPUs vary, so `config.toml` pins
  x86-64-v3 with an explicit `--target`. Wasmtime rejects a flag the host lacks,
  so an unsupported runner fails loudly rather than producing different code.

## Risks

* **Run time is unmeasured.** Estimated about 1 hour on 4 cores; the longest
  unit is about 25 minutes.
* **Pinned flags drift.** New Cranelift ISA flags stay off until added to
  `config.toml`.
* **Determinism.** Callgrind variance is near zero, so tiny changes are
  significant. A minimum effect size filters them (see
  [regressions.md](regressions.md)).
* **Repo growth.** Roughly 25 KiB per recorded run. At most 8 runs a day, so
  up to roughly 75 MiB per year if `main` changes before every run; less in
  practice, since unchanged commits are skipped.
* **Cron auto-disable.** GitHub disables cron workflows after 60 days without
  repo activity. Recorded data commits count.
