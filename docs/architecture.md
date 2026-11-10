# Architecture

A GitHub Actions run every 15 minutes benchmarks Wasmtime `main` under
Sightglass's callgrind measure, commits the raw results, and deploys trend
plots to GitHub Pages. For significant virtual-cycles regressions, it bisects
the run's range and files an issue on `bytecodealliance/wasmtime` naming the
commits responsible. A run that finds Wasmtime `main`
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

* **Three shards, many processes each.** Callgrind counts don't depend on
  load, so `awfy run` runs `nproc` units at once, each unit one iteration of
  one benchmark in its own process. A run is split across three runners by
  iteration: shard *k* runs iterations *k* and *k* + 3 of every benchmark. The
  engine and `sightglass-cli` are built once and shared, so every shard
  measures identical binaries.
* **Why three, by iteration.** One iteration of `starlingmonkey-markdown`
  takes 24 to 50 minutes, about half a run's CPU time, and no run can finish
  faster than one of them. Three shards reach that floor: two cores run
  starlingmonkey while the other two finish everything else. More shards, or
  splitting by benchmark too, only add setup and expose the run to more slow
  runners. Splitting by iteration also spreads each benchmark's samples across
  hosts, so a host effect shows up as spread the regression test can see
  rather than as a shift it would mistake for a Wasmtime change.
* **Pinned environment.** `.github/actions/install-valgrind` pins Valgrind,
  `rust-toolchain.toml` pins rustc, and the `sightglass/` submodule pins the
  suite and cost model. Changing any of them changes the fingerprint (see
  [data.md](data.md)).
* **Pinned Cranelift ISA flags.** Runner CPUs vary, so `config.toml` pins
  x86-64-v3 with an explicit `--target`. Wasmtime rejects a flag the host lacks,
  so an unsupported runner fails loudly rather than producing different code.

## Risks

* **Run time.** Unsharded runs took 60 to 130 minutes on 4 cores, depending
  on the runner's CPU. Sharded, a run takes about one starlingmonkey iteration
  plus about 5 minutes of setup per shard, so roughly 30 to 55 minutes. A run
  is as slow as its slowest shard.
* **Pinned flags drift.** New Cranelift ISA flags stay off until added to
  `config.toml`.
* **Determinism.** Callgrind variance is small but not zero: most benchmarks
  spread under 0.1% between processes, a few over 1%. Each run's noise is
  reported on its page and as error bars on the plots, and a minimum effect
  size filters out the tiny changes (see [regressions.md](regressions.md)).
* **Repo growth.** Roughly 45 KiB per recorded run. Runs one at a time and
  never measure the same commit twice, so there are at most as many runs as
  Wasmtime commits: about 3,900 a year at today's merge rate, or roughly
  165 MiB.
* **Cron auto-disable.** GitHub disables cron workflows after 60 days without
  repo activity. Recorded data commits count.
