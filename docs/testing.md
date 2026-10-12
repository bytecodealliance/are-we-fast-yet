# Testing

Workflows are hard to test, so they stay thin and the logic lives in `awfy`
and the site's JS.

* **Rust:** unit, snapshot, and integration tests against the fixtures in
  `crates/awfy/tests/fixtures/`. `AWFY_UPDATE_SNAPSHOTS=1 cargo test` rewrites
  snapshots. `awfy run` is tested with a fake `sightglass-cli`.
* **Smoke test:** `ci.yml` runs `awfy run` and `awfy record` on one cheap
  benchmark under real Valgrind with the real `config.toml` flags. This catches
  Sightglass output changes and runners that lack a pinned ISA flag.
* **Site:** `node --test` for pure modules and Playwright against the fixture
  site.
* **Workflows:** dispatch `benchmark.yml` from a branch with `dry-run` and a
  one-benchmark `suite`, then inspect the artifacts.
