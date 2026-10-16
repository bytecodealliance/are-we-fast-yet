## Regressions: run 00002 vs. 00001

Wasmtime `0123456789ab`..`ffffffffffff`.

A change counts when a Mann-Whitney U test finds it significant at the `0.01` level and it clears its phase's minimum relative change:

| phase | geomean | individual |
|-------|---------|------------|
| compilation | 2% | 1% |
| instantiation | 2% | 5% |
| execution | 0.2% | 1% |

### Regressions

No regressions.

### Suppressed (1)

Virtual cycles moved, but the delta is branch misses alone and the instruction count held still. That is Valgrind's branch predictors aliasing on a code layout change, not a regression; see `docs/regressions.md`.

| benchmark | phase | change | branch-miss share | instructions |
|-----------|-------|--------|-------------------|--------------|
| `a.wasm` | execution | +4.00% (1.040x, 1.040x to 1.040x) | 100.0% | +0.00% |

### Speedups

No speedups.

