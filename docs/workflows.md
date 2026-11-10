# Workflows

* `benchmark.yml`: run, record, push, and bisect regressions, every 15
  minutes. A `concurrency` group keeps one writer, so run numbers allocated at
  record time never collide, and a run that overruns its slot queues the next
  rather than racing it. The bisect starts after the push, so its failure never
  loses data. Its jobs:
  * `prepare` picks the Wasmtime commit, or skips if it was already measured,
    and builds `awfy`, the engine, and `sightglass-cli` once.
  * `measure` is a three-way matrix, one job per shard (see
    [architecture.md](architecture.md)), each running `awfy run --shard` with
    those binaries and uploading its scratch directory.
  * `record` passes every shard's scratch directory to `awfy record`, which
    refuses a run missing a shard, then pushes, checks for regressions, and
    starts `bisect.yml` if it finds any.
* `bisect.yml`: attribute a run's regressions to commits and file the issue.
  See [regressions.md](regressions.md). It has no `concurrency` group, so it
  never holds up the next benchmark run. `plan` writes the matrix, `measure`
  runs one job per candidate commit, and `report` runs even when those fail,
  so a failed bisect still files the range issue. Bisect jobs restore the
  Wasmtime build cache but never save it, so they don't evict the benchmark
  lane's entry.
* `pages.yml`: build and deploy the site. `benchmark.yml` calls it directly,
  since pushes made with `GITHUB_TOKEN` don't trigger workflows. Pages' source
  must be "GitHub Actions".
* `ci.yml`: see [testing.md](testing.md).

## Upgrades

Bumping Sightglass, the toolchain, Valgrind (the `version` default in
`.github/actions/install-valgrind`), or the runner image is a normal PR. It
changes the fingerprint, so the next run is not checked for regressions.
