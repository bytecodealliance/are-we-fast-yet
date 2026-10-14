# Site

`awfy site` copies `site/` and writes JSON to `data/`. The JSON is the contract
between Rust and JS; the snapshots in `crates/awfy/tests/snapshots/site/` pin
it.

* No build step: hand-written HTML, CSS, and ES modules, plus vendored uPlot.
* All view state is in the URL, so regression issues can link to any view.
* Pure JS logic is separate from DOM code so `node --test` covers it.
* A run with failed benchmarks gets no Geomean point, since a geomean over a
  different benchmark set is not comparable with its neighbors.
* Each point is the median of that run's samples, which is the statistic the
  regression test compares (see [regressions.md](regressions.md)), with an
  error bar whose full width is the run's noise: the interquartile range over
  the median, or the full range over it below 8 samples, where there are too
  few to set an outlier aside. The bars are omitted from the overview's small
  plots, where they would be a smear rather than a reading.
* The Geomean's bar means something else. A geomean aggregates the benchmarks'
  *mean* counts and has no samples of its own, so its bar is the standard error
  of `ln(geomean)` by the delta method, which shrinks as samples are added
  where a spread does not. The run page and the overview both say so.
* Noise is derived when the site is built, not stored; see [data.md](data.md).
* The run page lists the Wasmtime commits new since the previous run, read
  from that run's metadata (see [data.md](data.md)). Runs recorded without a
  log, such as the first one, show a note instead.
