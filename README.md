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
