# Workflows

Three workflows, all in this repo. Nothing is added to the Wasmtime repo.

## `benchmark.yml`

Triggers: daily `schedule` (off-peak, e.g. `17 3 * * *`) and
`workflow_dispatch` with optional inputs:

* `force`: run even when Wasmtime is unchanged.
* `dry-run`: skip pushing, filing issues, and deploying. Upload the run
  directory and would-be issue as workflow artifacts instead.
* `suite`: override the suite, e.g. a single cheap benchmark for testing.

```yaml
concurrency:
  group: benchmark
  cancel-in-progress: false
permissions:
  contents: write
```

One job on `ubuntu-24.04`, `timeout-minutes: 360`:

1. Check out this repo with submodules.
2. Resolve Wasmtime `main` to a SHA with `git ls-remote`. If it equals
   `awfy latest wasmtime.commit` and `force` is unset, stop.
3. Install Valgrind from apt.
4. Restore caches: `~/.cargo`, and a Wasmtime `target/` keyed on the toolchain.
5. Build `libengine.so` at that SHA with Sightglass's `engines/wasmtime/build`.
6. Build `sightglass-cli` and `awfy` in release mode.
7. `awfy run`: CSVs to a scratch directory.
8. `awfy record`: writes `benchmark-data/NNNNN/`.
9. Commit `Record run NNNNN (wasmtime <short-sha>)` and push to `main`. On a
   rejected push, rebase and retry. The concurrency group makes this rare.
10. `awfy regressions`: if it prints an issue, create it with a GitHub App
    token (see [regressions.md](regressions.md)).
11. Call `pages.yml`.

Step 10 runs after the push, so a failure there never loses data. A failed
step 10 fails the workflow, which emails maintainers through GitHub's normal
failed-workflow notifications.

## `pages.yml`

Triggers: `workflow_call` from `benchmark.yml`, `push` to `main` touching
`site/**`, `crates/**`, or `benchmark-data/excluded.toml`, and
`workflow_dispatch`.

```yaml
concurrency:
  group: pages
  cancel-in-progress: true
permissions:
  pages: write
  id-token: write
```

1. Check out this repo with submodules (full history is not needed).
2. Build `awfy`.
3. `awfy site --out _site`.
4. `actions/upload-pages-artifact` then `actions/deploy-pages`.

Pushes made with `GITHUB_TOKEN` do not trigger other workflows, so
`benchmark.yml` calls `pages.yml` directly.

Repo setting: Pages source is "GitHub Actions". There is no `gh-pages` branch.

## `ci.yml`

Triggers: `pull_request` and `push` to `main`. See [testing.md](testing.md).

## Upgrades

A Sightglass submodule bump, Rust toolchain bump, or runner image bump is a
normal PR. It changes the environment fingerprint, so the next run gets a UI
marker and is not checked for regressions. Before merging, run
`benchmark.yml` from the PR branch with `dry-run` to confirm it still works
end to end.
