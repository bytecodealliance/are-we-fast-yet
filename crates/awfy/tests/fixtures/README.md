# Test fixtures

A small repo root: `config.toml` and `benchmark-data/`. Two benchmarks, three
phases, three events (`virtual-cycles`, `instructions-retired`,
`l1-dcache-read-misses`), six iterations. Counts in each group are `base +
d*k` for `d` in `-10, -6, -2, 2, 6, 10`, where `k` is `base / 100000` (at
least 1), so the median is exactly `base`.

Six iterations rather than three because the comparison is a Mann-Whitney U
test over an exactly enumerated null distribution: with `n` samples of each run
there are only `C(2n, n)` ways for them to interleave, so no comparison can
report a p-value below `2 / C(2n, n)`. That floor is 0.1 at three samples,
which the 0.01 significance level in `config.toml` can never clear, and 0.0022
at six, which it can.

The fixture's `min-relative-change` values differ per phase and scope, so the
tests exercise picking the right one rather than a single threshold everywhere.

| Run | What |
|-----|------|
| 00001 | Baseline. No previous run, so no commit log. |
| 00002 | Identical to 00001: a steady pair. One commit logged, one omitted. |
| 00003 | Against 00002: `bz2` execution +1.5% (above execution's 1% individual threshold, with `instructions-retired` up too), `pulldown-cmark` compilation +0.5% (below compilation's), `bz2` compilation -3% (a speedup). The execution Geomean is +0.75%, above that phase's 0.2% geomean threshold; the compilation Geomean is -1.3%, below compilation's 2%. |
| 00004 | Every count doubled. Excluded in `excluded.toml`. |
| 00005 | Same counts as 00003, but `valgrind-3.24.0`: a fingerprint change. |
| 00006 | Same as 00005, but `pulldown-cmark` failed: no rows, and a `[failures]` entry. |

`sightglass.csv.gz` files are generated from the plain CSVs in `csv/`. After
editing one, regenerate with:

```sh
cargo test -p awfy --test fixtures -- --ignored
```

`cargo test` fails if a gzipped file is stale.
