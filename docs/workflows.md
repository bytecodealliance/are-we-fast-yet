# Workflows

* `benchmark.yml`: run, record, push, and file regressions, every hour. A
  `concurrency` group keeps one writer, so run numbers allocated at record
  time never collide, and a run that overruns its slot queues the next rather
  than racing it. Filing comes after the push, so its failure never loses
  data. Its jobs:
  * `prepare` picks the Wasmtime commit, or skips if it was already measured,
    and builds `awfy`, the engine, and `sightglass-cli` once.
  * `measure` is a three-way matrix, one job per shard (see
    [architecture.md](architecture.md)), each running `awfy run --shard` with
    those binaries and uploading its scratch directory.
  * `record` passes every shard's scratch directory to `awfy record`, which
    refuses a run missing a shard, then pushes, checks for regressions, and
    files the issue.
* `pages.yml`: build and deploy the site. `benchmark.yml` calls it directly,
  since pushes made with `GITHUB_TOKEN` don't trigger workflows. Pages' source
  must be "GitHub Actions".
* `ci.yml`: see [testing.md](testing.md).

## Upgrades

Bumping Sightglass, the toolchain, Valgrind (the `version` default in
`.github/actions/install-valgrind`), or the runner image is a normal PR. It
changes the fingerprint, so the next run is not checked for regressions.
