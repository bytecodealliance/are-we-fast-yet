# Test fixtures

A small repo root: `config.toml` and `benchmark-data/`. Two benchmarks, three
phases, three events (`virtual-cycles`, `instructions-retired`,
`l1-dcache-read-misses`), three iterations. Counts in each group are
`base - 10*k`, `base`, `base + 10*k`, where `k` is `base / 100000` (at least 1).

| Run | What |
|-----|------|
| 00001 | Baseline. |
| 00002 | Identical to 00001: a steady pair. |
| 00003 | Against 00002: `bz2` execution +1.5% (above the 1% threshold, with `instructions-retired` up too), `pulldown-cmark` compilation +0.5% (below), `bz2` compilation -3% (a speedup). |
| 00004 | Every count doubled. Excluded in `excluded.toml`. |
| 00005 | Same counts as 00003, but `valgrind-3.24.0`: a fingerprint change. |
| 00006 | Same as 00005, but `pulldown-cmark` failed: no rows, and a `[failures]` entry. |

`sightglass.csv.gz` files are generated from the plain CSVs in `csv/`. After
editing one, regenerate with:

```sh
cargo test -p awfy --test fixtures -- --ignored
```

`cargo test` fails if a gzipped file is stale.
