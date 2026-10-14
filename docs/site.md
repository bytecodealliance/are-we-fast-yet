# Site

`awfy site` copies `site/` and writes JSON to `data/`. The JSON is the contract
between Rust and JS; the snapshots in `crates/awfy/tests/snapshots/site/` pin
it.

* No build step: hand-written HTML, CSS, and ES modules, plus vendored uPlot.
* All view state is in the URL, so regression issues can link to any view.
* Pure JS logic is separate from DOM code so `node --test` covers it.
* A run with failed benchmarks gets no Geomean point, since a geomean over a
  different benchmark set is not comparable with its neighbors.
* The run page lists the Wasmtime commits new since the previous run, read
  from that run's metadata (see [data.md](data.md)). Runs recorded without a
  log, such as the first one, show a note instead.
