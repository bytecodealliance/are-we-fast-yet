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

A `benchmark` job on `ubuntu-24.04`, `timeout-minutes: 360`, with
`contents: write` and `issues: write` (see [tokens.md](tokens.md)):

1. Check out this repo with submodules. Non-dry runs must be on `main`.
2. Build `awfy`. Resolve Wasmtime `main` to a SHA with `git ls-remote`. If it
   equals `awfy latest wasmtime.commit` and `force` is unset, skip the rest
   (the run succeeds).
3. Treeless-clone Wasmtime (`git clone --filter=tree:0 --no-checkout`) for the
   commit date (`git log -1 --format=%cI`) and the issue's commit log.
4. Install Valgrind from apt.
5. Restore a cache of Sightglass's Wasmtime build directory (clone and
   `target/`), keyed on `rust-toolchain.toml` with a prefix fallback.
6. Build `libengine.so` at that SHA with Sightglass's `engines/wasmtime/build.rs`,
   `BUILD_DIR` set to the cached directory and `RUSTUP_TOOLCHAIN` to the pinned
   toolchain. The script fetches into the existing clone, so builds are
   incremental. The step checks that the clone's `HEAD` is the SHA.
7. Build `sightglass-cli` in release mode.
8. `awfy run`: CSVs to a scratch directory.
9. `awfy record`: writes `benchmark-data/NNNNN/` and prints `NNNNN`.
10. Commit `Record run NNNNN (wasmtime <short-sha>)` as `github-actions[bot]`
    and push to `main`. On a rejected push, `git pull --rebase` and retry, up
    to 3 times. The concurrency group makes this rare.
11. `awfy regressions --issue-dir`: if it writes an issue, search
    `regressions.issue-repo` (from `awfy config`) for a bot-authored issue
    whose title ends in `awfy issue-key`'s suffix, and if there is none, file
    it with `gh issue create`. The token is `GITHUB_TOKEN` when the issue repo
    is this repo, and otherwise a GitHub App token scoped to the issue repo
    (see [regressions.md](regressions.md)).

On a dry run, step 10 and filing are skipped, and the run directory and issue
directory are uploaded as an artifact instead.

A second job, `pages`, calls `pages.yml` with `ref: main` (the triggering
commit predates the pushed data). It needs `pages: write` and
`id-token: write`, and is skipped on dry runs and skipped runs.

Step 11 runs after the push, so a failure there never loses data. A failed
step 11 fails the workflow, which emails maintainers through GitHub's normal
failed-workflow notifications.

## `pages.yml`

Triggers: `workflow_call` from `benchmark.yml`, `push` to `main` touching
`site/**`, `crates/**`, `benchmark-data/excluded.toml`, or `config.toml`, and
`workflow_dispatch`.

```yaml
concurrency:
  group: pages
  cancel-in-progress: true
permissions:
  contents: read
  pages: write
  id-token: write
```

The job uses the `github-pages` environment.

1. Check out this repo with submodules (full history is not needed), at the
   `ref` input when called.
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
