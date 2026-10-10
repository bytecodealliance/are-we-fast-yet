# Testing

Workflows are hard to test, so they stay thin: check out, install, build, call
`awfy`, push. Almost all logic is in `awfy` and the site's JS, which are tested
locally and in `ci.yml`. The workflows themselves get end-to-end coverage from
dry runs.

## Unit tests (Rust, `cargo test`)

| Area | Covers |
|------|--------|
| Run numbering | Empty dir gives `00001`. Gaps and stray files are ignored. Five-digit padding. |
| Metadata | TOML round trip. Rejects unknown `schema`. Fingerprint equality and change descriptions. |
| `record` | Merges per-benchmark CSVs. Rewrites `engine` and `wasm` columns. Lists failures. Output is valid gzip. |
| `run` | Builds the right `sightglass-cli` command lines from `config.toml`. Collects failures without aborting. (Processes are behind a trait and faked.) |
| Baseline selection | Skips excluded runs. No baseline, fingerprint mismatch, and equal-commit cases. |
| Regression filter | Significance and `min-relative-change` interplay, speedups vs. regressions, missing benchmarks. |
| Issue rendering | Snapshot tests of title and body for: one regression, many, Geomean, truncated commit log. |
| Site JSON | Snapshot tests of `index.json` and per-benchmark JSON from fixture runs. |

Snapshot tests compare output with files in `crates/awfy/tests/snapshots/`,
using a small helper in `awfy` rather than `insta`, to keep dependencies few.
`AWFY_UPDATE_SNAPSHOTS=1 cargo test` rewrites them; review the diff.

## Fixtures

`crates/awfy/tests/fixtures/benchmark-data/`: a few small, hand-written runs
(2 benchmarks, 3 phases, a few events), including:

* a steady pair,
* a pair with a regression just above and one just below the threshold,
* a fingerprint change,
* an excluded run,
* a run with a failed benchmark.

## Integration tests (Rust, `cargo test`)

Run the `awfy` binary end to end against a temp copy of the fixtures:

* `awfy record` then `awfy regressions` then `awfy site`, asserting the
  resulting files and stdout.
* `awfy run` with a fake `sightglass-cli` (`examples/fake-sightglass-cli.rs`,
  passed via `--sightglass-cli`, that writes canned CSV), to test
  orchestration, parallelism, and failure handling without Valgrind.

## Real Sightglass smoke test (`ci.yml`, Linux only)

Builds `libengine.so` from a pinned Wasmtime release (cached, like Sightglass's
own CI), then runs `awfy run` and `awfy record` on `benchmarks/noop` with one
process under real Valgrind. Asserts the CSV has every callgrind event for
every phase and that metadata fields are filled in. Catches Sightglass
submodule bumps that change output format or event names.

Uses the real `config.toml` engine flags, so it also checks that Valgrind on the
runner supports every pinned Cranelift ISA flag. Wasmtime rejects an enabled
flag the host lacks, so the test fails on an unsupported runner.

## Site tests

* `node --test` unit tests for pure JS modules: URL state parsing, relative
  normalization, series lookup.
* A Playwright test loads `awfy site` output from fixtures via a local static
  server and checks that each page renders a canvas, without console errors.
  It also follows a link from a rendered regression issue snapshot to its
  benchmark page.

## `ci.yml`

Triggers: `pull_request`, `push` to `main`.

1. `cargo fmt --check`, `cargo clippy -D warnings`.
2. `cargo test` (unit and integration).
3. Real Sightglass smoke test.
4. Site tests.
5. `actionlint` on `.github/workflows/`.
6. Data lint: every `benchmark-data/*/metadata.toml` parses and every CSV
   decompresses and parses. Guards against bad hand edits.

## Testing the workflows

* **PRs touching workflows:** run `benchmark.yml` from the branch via
  `workflow_dispatch` with `dry-run` and a one-benchmark `suite`. Takes minutes.
  Inspect the uploaded run directory and would-be issue.
* **Issue filing:** during rollout, `issue-repo` is this repo, so real issues
  land here before ever touching Wasmtime.
* **Pages:** `pages.yml` is idempotent and side-effect free apart from the
  deploy; run it on demand.
* **Local:** `act` is not required. Every workflow step is a shell command or
  `awfy` call that also runs on a Linux machine with Valgrind.
