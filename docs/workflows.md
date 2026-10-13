# Workflows

* `benchmark.yml`: run, record, push, and file regressions, every three
  hours. A `concurrency` group keeps one writer, so run numbers allocated at
  record time never collide, and a run that overruns its slot queues the next
  rather than racing it. Filing comes after the push, so its failure never
  loses data.
* `pages.yml`: build and deploy the site. `benchmark.yml` calls it directly,
  since pushes made with `GITHUB_TOKEN` don't trigger workflows. Pages' source
  must be "GitHub Actions".
* `ci.yml`: see [testing.md](testing.md).

## Upgrades

Bumping Sightglass, the toolchain, Valgrind (the `version` default in
`.github/actions/install-valgrind`), or the runner image is a normal PR. It
changes the fingerprint, so the next run is not checked for regressions.
