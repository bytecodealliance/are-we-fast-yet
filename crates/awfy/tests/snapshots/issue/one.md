Performance regression in 222222222222..333333333333 (are-we-fast-yet run 00003)

[are-we-fast-yet](https://github.com/bytecodealliance/are-we-fast-yet) found 2 regressions in virtual cycles (callgrind) between Wasmtime `222222222222` and `333333333333`.

* Baseline: [run 00002](https://bytecodealliance.github.io/are-we-fast-yet/run.html?run=00002) ([metadata](https://github.com/bytecodealliance/are-we-fast-yet/blob/main/benchmark-data/00002/metadata.toml))
* This run: [run 00003](https://bytecodealliance.github.io/are-we-fast-yet/run.html?run=00003) ([metadata](https://github.com/bytecodealliance/are-we-fast-yet/blob/main/benchmark-data/00003/metadata.toml))
* Threshold: significant at the 0.01 level, and at least this much more:

| phase | geomean | individual |
|-------|---------|------------|
| compilation | 2% | 1% |
| instantiation | 2% | 5% |
| execution | 0.2% | 1% |

| benchmark | phase | **Δ virtual cycles** | virtual cycles noise | Δ instructions retired | instructions retired noise |
|-----------|-------|----------------------|----------------------|------------------------|----------------------------|
| [`Geomean`](https://bytecodealliance.github.io/are-we-fast-yet/benchmark.html?b=Geomean&metric=virtual-cycles) | execution | **+0.75% (+0.74% to +0.76%)** | n/a | n/a | n/a |
| [`bz2/benchmark.wasm`](https://bytecodealliance.github.io/are-we-fast-yet/benchmark.html?b=bz2-benchmark&metric=virtual-cycles) | execution | **+1.50% (+1.48% to +1.52%)** | 0.020% -> 0.020% | +1.20% | 0.020% -> 0.020% |

A positive delta is a slowdown, a negative delta is a speedup.

`instructions retired` counts work done rather than how well it was predicted. A regression whose instructions didn't move is often Valgrind's branch predictors aliasing on a code layout change; see [`docs/regressions.md`](https://github.com/bytecodealliance/are-we-fast-yet/blob/main/docs/regressions.md).

Each noise column is how far that event's own counts spread.

<details>
<summary>Other callgrind counters that changed for these benchmarks</summary>

| benchmark | phase | counter | Δ | noise |
|-----------|-------|---------|---|-------|
| `Geomean` | execution | `instructions-retired` | +0.60% (+0.59% to +0.61%) | n/a |
| `bz2/benchmark.wasm` | execution | `instructions-retired` | +1.20% (+1.18% to +1.22%) | 0.020% -> 0.020% |

</details>

### Commits

https://github.com/bytecodealliance/wasmtime/compare/2222222222222222222222222222222222222222...3333333333333333333333333333333333333333

* [`abc0002fffff` winch: fix the stack alignment on aarch64](https://github.com/bytecodealliance/wasmtime/commit/abc0002fffffffffffffffffffffffffffffffff)
* [`abc0001fffff` Revert "Bump the MSRV to 1.84"](https://github.com/bytecodealliance/wasmtime/commit/abc0001fffffffffffffffffffffffffffffffff)
* [`abc0000fffff` Cranelift: add an egraph rule for `x * 2`](https://github.com/bytecodealliance/wasmtime/commit/abc0000fffffffffffffffffffffffffffffffff)

### Reproduce locally

On x86-64 Linux with Valgrind installed:

```sh
git clone https://github.com/bytecodealliance/sightglass
cd sightglass
git checkout e38856287c8871228d7ff843e29028c381a71dd7
rustc engines/wasmtime/build.rs -o build-engine
mkdir old new
REVISION=2222222222222222222222222222222222222222 ./build-engine old
REVISION=3333333333333333333333333333333333333333 ./build-engine new
cargo run --release -- benchmark --measure callgrind \
  --engine old/libengine.so --engine new/libengine.so \
  --engine-flags-common="--target x86_64-unknown-linux-gnu -Ccranelift-has-avx2" \
  -- benchmarks/bz2/benchmark.wasm
```
