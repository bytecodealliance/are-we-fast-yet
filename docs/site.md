# Site

Static files deployed to GitHub Pages by `pages.yml`. No server and no JS build
step: hand-written HTML, CSS, and ES modules, plus one vendored charting
library.

## Build

`awfy site --out _site`:

1. Copies `site/` to `_site/`, except development files (`package.json`,
   `node_modules/`, `tests/`, ...). `--out` is removed first.
2. Reads every non-excluded run in `benchmark-data/`.
3. Writes the JSON below to `_site/data/`.

## Generated JSON

The data contract between Rust and JS. Golden-tested (see
[testing.md](testing.md)).

`data/index.json`:

```json
{
  "schema": 1,
  "generated": "2026-10-10T04:30:00Z",
  "repo": "bytecodealliance/are-we-fast-yet",
  "runs": [
    {
      "run": 42,
      "wasmtime": "0123abc...",
      "date": "2026-10-09T22:41:05Z",
      "fingerprint-change": null
    }
  ],
  "benchmarks": ["bz2/benchmark.wasm", "..."],
  "phases": ["compilation", "instantiation", "execution"],
  "metrics": ["virtual-cycles", "instructions-retired", "..."]
}
```

`fingerprint-change` is null, or a short description of what changed since the
previous run (e.g. `"valgrind 3.22.0 -> 3.24.0"`).

`repo` is `config.toml`'s `site.repo`, for links to raw data. `benchmarks`
lists Geomean first, then the rest sorted. `metrics` lists
`virtual-cycles` first. `phases` is in lifecycle order.

`data/benchmarks/<slug>.json`, one per benchmark plus Geomean. The slug drops
`.wasm`, turns `/` into `-`, and any other character outside
`[A-Za-z0-9._-]` into `_`, e.g. `bz2-benchmark`. Rust (`awfy::slug`) and JS
(`site/js/slug.js`) share test cases.

```json
{
  "benchmark": "bz2/benchmark.wasm",
  "series": {
    "compilation": {
      "virtual-cycles": [[42, 123456.0], [43, 123460.0]]
    }
  }
}
```

Points are `[run, mean]` across processes and iterations. The run indexes into
`index.json` for date and commit. Callgrind spread is negligible, so min/max
are omitted.

Raw CSVs have no Geomean rows. `awfy site` computes them per run with
`sightglass_analysis::geomean::add`, the same as regression detection. A run
with failed benchmarks has no Geomean point, since a geomean over a different
benchmark set is not comparable with its neighbors.

Means are integers when integral and otherwise rounded to 2 decimal places.

`data/runs/<NNNNN>.json`, one per run, for `run.html`:

```json
{
  "run": 3,
  "previous": 2,
  "fingerprint-change": null,
  "metadata": { "schema": 1, "started": "2026-10-10T03:17:00Z", "...": "..." },
  "changes": [
    {
      "benchmark": "bz2/benchmark.wasm",
      "phase": "execution",
      "mean": 2030000,
      "previous": 2000000,
      "change": 0.015
    }
  ]
}
```

`previous` is the previous visible run. `metadata` is `metadata.toml` as
JSON. `changes` has the virtual-cycles mean of every (benchmark, phase) in the
run, Geomean first; `previous` and `change` (relative, `(mean - previous) /
previous`) are null when the previous run lacks that benchmark.

Size estimate: 14 metrics x 3 phases x 365 runs is about 15k points per
benchmark per year, roughly 200 KiB of JSON before Pages' gzip.

`generated` comes from `--now` or `SOURCE_DATE_EPOCH` when set, so tests get
stable output.

## Pages

All state is in the URL query string so every view is linkable, e.g. from
regression issues.

* **`index.html`**: Overview. Phase and metric selectors (default:
  execution, virtual cycles). Large Geomean plot on top, then a grid of small
  plots, one per benchmark. Each links to its benchmark page.
* **`benchmark.html?b=<slug>&metric=<m>`**: One plot per phase for the chosen
  metric.
* **`run.html?run=NNNNN`**: That run's metadata, links to its raw files on
  GitHub, and a table of every (benchmark, phase) with change from the previous
  run.

## Plots

* Scatter plots. x: Wasmtime commit date. y: metric.
* Toggle between absolute values and relative to the first visible point.
* Vertical marker at each fingerprint change, labeled with the change.
* Hover: value, run, Wasmtime short SHA. Click: opens `run.html` for that run.
* Drag to zoom on x; the range is kept in the URL.

## Code

`site/js/` holds ES modules. Pure logic (`state.js`, `data.js`,
`transform.js`, `slug.js`) is separate from DOM and uPlot code (`plot.js`,
`page.js`, and one `*-page.js` per page), so it is tested with `node --test`.
`site/package.json` holds dev dependencies only (Playwright); nothing deployed
uses `node_modules`.

## Charting library

uPlot, vendored as a single minified file under `site/vendor/` with its license.
It is small, fast with thousands of points, and needs no build step. Vendoring
avoids runtime CDN dependencies.
