# Architecture

Continuous Sightglass benchmarking for Wasmtime and Cranelift. Agreed scope:
[wasmtime#13576](https://github.com/bytecodealliance/wasmtime/issues/13576#issuecomment-4674035049).

## Goals

1. **Long-term trends.** A daily run of the latest Wasmtime `main` under
   Sightglass's `callgrind` measure, plotted over time on a static site.
2. **Regression alerts.** Each new run is compared with the previous one. An
   issue is filed on `bytecodealliance/wasmtime` for statistically significant
   regressions in virtual cycles.

## Non-goals (for now)

* Wall-clock or `perf` counter measurements. GitHub runners are too noisy.
* Dedicated hardware, servers, databases, S3, CDNs.
* Benchmarking individual PRs on request.
* Backfilling history for old Wasmtime commits.

## Principles

* Everything lives in git: raw data, code, workflows, site sources.
* GitHub Actions is the only compute. GitHub Pages is the only hosting.
* Workflow YAML stays thin. Logic lives in one Rust CLI, `awfy`, so it runs and
  tests locally.
* Raw data is the source of truth. All derived data is regenerated at deploy
  time and never committed.
* No changes to the Wasmtime repo. Filing issues there needs only a GitHub App
  installation (see [tokens.md](tokens.md)).

## Components

| Component | What it is |
|-----------|------------|
| `benchmark-data/` | One directory per run: raw Sightglass CSV and metadata. See [data.md](data.md). |
| `crates/awfy/` | Rust CLI: run benchmarks, record a run, detect regressions, generate site data. |
| `sightglass/` | Git submodule. The single pin for Sightglass, used both as a Cargo path dependency and to build `sightglass-cli`. |
| `site/` | Static HTML/CSS/JS for the UI. See [site.md](site.md). |
| `config.toml` | Run configuration: suite, engine flags, process count, regression threshold. |
| `.github/workflows/` | `benchmark.yml`, `pages.yml`, `ci.yml`. See [workflows.md](workflows.md). |

## Data flow

```
daily cron
  -> benchmark.yml
       resolve wasmtime main -> SHA (skip if unchanged since last run)
       build libengine.so, sightglass-cli, awfy
       awfy run         -> per-benchmark CSVs (callgrind, parallel on one runner)
       awfy record      -> benchmark-data/NNNNN/{sightglass.csv.gz, metadata.toml}
       git commit + push to main
       awfy regressions -> issue on bytecodealliance/wasmtime, if any
  -> pages.yml (called)
       awfy site        -> _site/ (static files + generated JSON)
       actions/deploy-pages
```

## Repository layout

```
.github/workflows/{benchmark,pages,ci}.yml
benchmark-data/00001/{sightglass.csv.gz,metadata.toml}
crates/awfy/
site/
sightglass/            (submodule)
config.toml
rust-toolchain.toml
docs/
```

## `awfy` subcommands

| Subcommand | Purpose |
|------------|---------|
| `awfy latest <field>` | Print a field from the latest run's metadata, e.g. to skip runs when Wasmtime is unchanged. |
| `awfy run` | Run the suite under callgrind. Each (benchmark, process) pair is its own `sightglass-cli benchmark --processes 1` invocation, `nproc` at a time. |
| `awfy record` | Merge per-benchmark CSVs, gzip, gather metadata, allocate the next run number, write the run directory. |
| `awfy regressions` | Compare a run with its predecessor. Emit an issue title and body, or nothing. |
| `awfy site` | Copy `site/` and generate JSON summaries into an output directory. |

`awfy` uses `sightglass-data` and `sightglass-analysis` from the submodule as
path dependencies, so it parses CSVs and computes effect sizes with exactly the
code that recorded them.

## Key decisions

* **Parallel on one runner, not sharded across runners.** Callgrind counts do
  not depend on concurrent load, so running `nproc` callgrind processes at once
  is safe and gives one host per run (one `uname -a`). Small work units balance
  load, and one failing benchmark does not lose the rest. If a run outgrows the
  6 hour job limit, shard across a job matrix and make `uname` per shard.
* **Run numbers are allocated at commit time.** A `concurrency` group serializes
  benchmark workflows, so only one writer touches `benchmark-data/`.
* **Pinned environment.** `runs-on: ubuntu-24.04` (not `-latest`) pins the
  Valgrind version. `rust-toolchain.toml` plus `RUSTUP_TOOLCHAIN` pins the
  rustc that builds Wasmtime. The Sightglass submodule pins the suite, the cache
  model, and the virtual cycles formula. Cranelift ISA flags are pinned (see
  below). Changing any of these is an
  *environment change*, recorded in metadata and surfaced in the UI and in
  regression detection.
* **Pinned Cranelift ISA flags.** Runner CPUs vary between runs (Intel vs. AMD,
  differing extensions), so host feature detection would change generated code
  from run to run. `config.toml` pins them via engine flags:

  ```toml
  [benchmark]
  # x86-64-v3, the newest level Valgrind's synthetic CPU supports.
  engine-flags = """--target x86_64-unknown-linux-gnu \
    -Ccranelift-has-sse3 -Ccranelift-has-ssse3 -Ccranelift-has-sse41 \
    -Ccranelift-has-sse42 -Ccranelift-has-popcnt -Ccranelift-has-avx \
    -Ccranelift-has-avx2 -Ccranelift-has-fma -Ccranelift-has-bmi1 \
    -Ccranelift-has-bmi2 -Ccranelift-has-lzcnt -Ccranelift-has-cmpxchg16b"""
  ```

  An explicit `--target` disables Wasmtime's native feature inference, so the
  listed flags are the complete set. Wasmtime still checks enabled flags
  against the host at engine creation, so a runner lacking one fails loudly
  instead of silently producing different code. The host CPU is recorded in
  metadata but not fingerprinted.

## Risks and open questions

* **Run time is unmeasured.** Rough estimate for `default.suite` (the
  PCA 20-minute suite, 29 benchmarks) under callgrind with 3 processes: about 3
  hours serial, about 1 hour across a standard runner's 4 cores. The longest
  single unit (StarlingMonkey compilation) is about 25 minutes. Measure this
  first.
* **Pinned ISA level vs. Valgrind.** The pinned flags assume Valgrind reports
  x86-64-v3 on every runner. The smoke test (see [testing.md](testing.md))
  verifies this. If a runner ever lacks a feature, the run fails, and we lower
  the pinned level.
* **Pinned flags drift from Wasmtime.** New Cranelift ISA flags are off by
  default, so pinning silently ignores them until we add them. Adding one is a
  `config.toml` change and therefore a fingerprint change.
* **Determinism.** Callgrind with ASLR disabled is near-deterministic, so
  variance is near zero and tiny changes test as significant. Regression
  detection therefore also requires a minimum effect size. See
  [regressions.md](regressions.md).
* **Repo growth.** About 3.7k CSV rows per run, an estimated 50-100 KiB gzipped,
  so roughly 20-40 MiB per year. Acceptable; revisit if the suite grows a lot.
* **Scheduled workflow auto-disable.** GitHub disables cron workflows in public
  repos after 60 days without activity. Daily data commits count as activity.
