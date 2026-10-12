# Are we fast yet?

Continuous [Sightglass](https://github.com/bytecodealliance/sightglass)
benchmarking for Wasmtime and Cranelift. A daily GitHub Actions run measures
the latest Wasmtime `main` under callgrind, commits the raw results to
`benchmark-data/`, files an issue for significant regressions, and publishes
trend plots to GitHub Pages. See [`docs/`](docs/README.md) for the design.

## Build and test

```sh
git submodule update --init
cargo build
cargo test
```

The CLI is `awfy`; run `cargo run -p awfy -- --help` for its subcommands.

## View the site locally

```sh
cargo run -p awfy -- site --out _site
node site/tests/e2e/serve.js _site 8000   # or any static file server
```

Then open <http://127.0.0.1:8000/>. With no runs recorded yet, use the test
fixtures instead:

```sh
cargo run -p awfy -- --root crates/awfy/tests/fixtures site --site site --out _site
```

## Site tests

```sh
cd site
node --test tests/unit
npm ci && npx playwright install chromium && npx playwright test
```
