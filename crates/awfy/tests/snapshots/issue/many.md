Performance regression in 222222222222..333333333333 (are-we-fast-yet run 00003)

[are-we-fast-yet](https://github.com/bytecodealliance/are-we-fast-yet) found 4 regressions in virtual cycles (callgrind) between Wasmtime `222222222222` and `333333333333`.

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
| [`Geomean`](https://bytecodealliance.github.io/are-we-fast-yet/benchmark.html?b=Geomean&metric=virtual-cycles) | execution | **+5.00% (+4.60% to +5.40%)** | n/a | n/a | n/a |
| [`spidermonkey/spidermonkey-json.wasm`](https://bytecodealliance.github.io/are-we-fast-yet/benchmark.html?b=spidermonkey-spidermonkey-json&metric=virtual-cycles) | compilation | **+10.00% (+9.60% to +10.40%)** | 0.012% -> 0.015% | +8.00% | 0.009% -> 0.011% |
| [`bz2/benchmark.wasm`](https://bytecodealliance.github.io/are-we-fast-yet/benchmark.html?b=bz2-benchmark&metric=virtual-cycles) | execution | **+1.50% (+1.10% to +1.90%)** | 0.012% -> 0.015% | +1.20% | 0.009% -> 0.011% |
| [`bz2/benchmark.wasm`](https://bytecodealliance.github.io/are-we-fast-yet/benchmark.html?b=bz2-benchmark&metric=virtual-cycles) | compilation | **+2.00% (+1.60% to +2.40%)** | 0.012% -> 0.015% | +1.60% | 0.009% -> 0.011% |

A positive delta is a slowdown, a negative delta is a speedup.

`instructions retired` counts work done rather than how well it was predicted. A regression whose instructions didn't move is often Valgrind's branch predictors aliasing on a code layout change; see [`docs/regressions.md`](https://github.com/bytecodealliance/are-we-fast-yet/blob/main/docs/regressions.md).

Each noise column is how far that event's own counts spread.

<details>
<summary>Other callgrind counters that changed for these benchmarks</summary>

| benchmark | phase | counter | Δ | noise |
|-----------|-------|---------|---|-------|
| `bz2/benchmark.wasm` | execution | `l1-dcache-read-misses` | +100.00% (+99.60% to +100.40%) | 0.012% -> 0.015% |
| `bz2/benchmark.wasm` | execution | `conditional-branches` | -10.00% (-10.32% to -9.67%) | 0.012% -> 0.015% |

</details>

### Commits

https://github.com/bytecodealliance/wasmtime/compare/2222222222222222222222222222222222222222...3333333333333333333333333333333333333333

* [`abc003bfffff` Bump wasm-tools to 1.220.0](https://github.com/bytecodealliance/wasmtime/commit/abc003bfffffffffffffffffffffffffffffffff)
* [`abc003afffff` winch: fix the stack alignment on aarch64](https://github.com/bytecodealliance/wasmtime/commit/abc003afffffffffffffffffffffffffffffffff)
* [`abc0039fffff` Revert "Bump the MSRV to 1.84"](https://github.com/bytecodealliance/wasmtime/commit/abc0039fffffffffffffffffffffffffffffffff)
* [`abc0038fffff` Cranelift: add an egraph rule for `x * 2`](https://github.com/bytecodealliance/wasmtime/commit/abc0038fffffffffffffffffffffffffffffffff)
* [`abc0037fffff` Bump wasm-tools to 1.220.0](https://github.com/bytecodealliance/wasmtime/commit/abc0037fffffffffffffffffffffffffffffffff)
* [`abc0036fffff` winch: fix the stack alignment on aarch64](https://github.com/bytecodealliance/wasmtime/commit/abc0036fffffffffffffffffffffffffffffffff)
* [`abc0035fffff` Revert "Bump the MSRV to 1.84"](https://github.com/bytecodealliance/wasmtime/commit/abc0035fffffffffffffffffffffffffffffffff)
* [`abc0034fffff` Cranelift: add an egraph rule for `x * 2`](https://github.com/bytecodealliance/wasmtime/commit/abc0034fffffffffffffffffffffffffffffffff)
* [`abc0033fffff` Bump wasm-tools to 1.220.0](https://github.com/bytecodealliance/wasmtime/commit/abc0033fffffffffffffffffffffffffffffffff)
* [`abc0032fffff` winch: fix the stack alignment on aarch64](https://github.com/bytecodealliance/wasmtime/commit/abc0032fffffffffffffffffffffffffffffffff)
* [`abc0031fffff` Revert "Bump the MSRV to 1.84"](https://github.com/bytecodealliance/wasmtime/commit/abc0031fffffffffffffffffffffffffffffffff)
* [`abc0030fffff` Cranelift: add an egraph rule for `x * 2`](https://github.com/bytecodealliance/wasmtime/commit/abc0030fffffffffffffffffffffffffffffffff)
* [`abc002ffffff` Bump wasm-tools to 1.220.0](https://github.com/bytecodealliance/wasmtime/commit/abc002ffffffffffffffffffffffffffffffffff)
* [`abc002efffff` winch: fix the stack alignment on aarch64](https://github.com/bytecodealliance/wasmtime/commit/abc002efffffffffffffffffffffffffffffffff)
* [`abc002dfffff` Revert "Bump the MSRV to 1.84"](https://github.com/bytecodealliance/wasmtime/commit/abc002dfffffffffffffffffffffffffffffffff)
* [`abc002cfffff` Cranelift: add an egraph rule for `x * 2`](https://github.com/bytecodealliance/wasmtime/commit/abc002cfffffffffffffffffffffffffffffffff)
* [`abc002bfffff` Bump wasm-tools to 1.220.0](https://github.com/bytecodealliance/wasmtime/commit/abc002bfffffffffffffffffffffffffffffffff)
* [`abc002afffff` winch: fix the stack alignment on aarch64](https://github.com/bytecodealliance/wasmtime/commit/abc002afffffffffffffffffffffffffffffffff)
* [`abc0029fffff` Revert "Bump the MSRV to 1.84"](https://github.com/bytecodealliance/wasmtime/commit/abc0029fffffffffffffffffffffffffffffffff)
* [`abc0028fffff` Cranelift: add an egraph rule for `x * 2`](https://github.com/bytecodealliance/wasmtime/commit/abc0028fffffffffffffffffffffffffffffffff)
* [`abc0027fffff` Bump wasm-tools to 1.220.0](https://github.com/bytecodealliance/wasmtime/commit/abc0027fffffffffffffffffffffffffffffffff)
* [`abc0026fffff` winch: fix the stack alignment on aarch64](https://github.com/bytecodealliance/wasmtime/commit/abc0026fffffffffffffffffffffffffffffffff)
* [`abc0025fffff` Revert "Bump the MSRV to 1.84"](https://github.com/bytecodealliance/wasmtime/commit/abc0025fffffffffffffffffffffffffffffffff)
* [`abc0024fffff` Cranelift: add an egraph rule for `x * 2`](https://github.com/bytecodealliance/wasmtime/commit/abc0024fffffffffffffffffffffffffffffffff)
* [`abc0023fffff` Bump wasm-tools to 1.220.0](https://github.com/bytecodealliance/wasmtime/commit/abc0023fffffffffffffffffffffffffffffffff)
* [`abc0022fffff` winch: fix the stack alignment on aarch64](https://github.com/bytecodealliance/wasmtime/commit/abc0022fffffffffffffffffffffffffffffffff)
* [`abc0021fffff` Revert "Bump the MSRV to 1.84"](https://github.com/bytecodealliance/wasmtime/commit/abc0021fffffffffffffffffffffffffffffffff)
* [`abc0020fffff` Cranelift: add an egraph rule for `x * 2`](https://github.com/bytecodealliance/wasmtime/commit/abc0020fffffffffffffffffffffffffffffffff)
* [`abc001ffffff` Bump wasm-tools to 1.220.0](https://github.com/bytecodealliance/wasmtime/commit/abc001ffffffffffffffffffffffffffffffffff)
* [`abc001efffff` winch: fix the stack alignment on aarch64](https://github.com/bytecodealliance/wasmtime/commit/abc001efffffffffffffffffffffffffffffffff)
* [`abc001dfffff` Revert "Bump the MSRV to 1.84"](https://github.com/bytecodealliance/wasmtime/commit/abc001dfffffffffffffffffffffffffffffffff)
* [`abc001cfffff` Cranelift: add an egraph rule for `x * 2`](https://github.com/bytecodealliance/wasmtime/commit/abc001cfffffffffffffffffffffffffffffffff)
* [`abc001bfffff` Bump wasm-tools to 1.220.0](https://github.com/bytecodealliance/wasmtime/commit/abc001bfffffffffffffffffffffffffffffffff)
* [`abc001afffff` winch: fix the stack alignment on aarch64](https://github.com/bytecodealliance/wasmtime/commit/abc001afffffffffffffffffffffffffffffffff)
* [`abc0019fffff` Revert "Bump the MSRV to 1.84"](https://github.com/bytecodealliance/wasmtime/commit/abc0019fffffffffffffffffffffffffffffffff)
* [`abc0018fffff` Cranelift: add an egraph rule for `x * 2`](https://github.com/bytecodealliance/wasmtime/commit/abc0018fffffffffffffffffffffffffffffffff)
* [`abc0017fffff` Bump wasm-tools to 1.220.0](https://github.com/bytecodealliance/wasmtime/commit/abc0017fffffffffffffffffffffffffffffffff)
* [`abc0016fffff` winch: fix the stack alignment on aarch64](https://github.com/bytecodealliance/wasmtime/commit/abc0016fffffffffffffffffffffffffffffffff)
* [`abc0015fffff` Revert "Bump the MSRV to 1.84"](https://github.com/bytecodealliance/wasmtime/commit/abc0015fffffffffffffffffffffffffffffffff)
* [`abc0014fffff` Cranelift: add an egraph rule for `x * 2`](https://github.com/bytecodealliance/wasmtime/commit/abc0014fffffffffffffffffffffffffffffffff)
* [`abc0013fffff` Bump wasm-tools to 1.220.0](https://github.com/bytecodealliance/wasmtime/commit/abc0013fffffffffffffffffffffffffffffffff)
* [`abc0012fffff` winch: fix the stack alignment on aarch64](https://github.com/bytecodealliance/wasmtime/commit/abc0012fffffffffffffffffffffffffffffffff)
* [`abc0011fffff` Revert "Bump the MSRV to 1.84"](https://github.com/bytecodealliance/wasmtime/commit/abc0011fffffffffffffffffffffffffffffffff)
* [`abc0010fffff` Cranelift: add an egraph rule for `x * 2`](https://github.com/bytecodealliance/wasmtime/commit/abc0010fffffffffffffffffffffffffffffffff)
* [`abc000ffffff` Bump wasm-tools to 1.220.0](https://github.com/bytecodealliance/wasmtime/commit/abc000ffffffffffffffffffffffffffffffffff)
* [`abc000efffff` winch: fix the stack alignment on aarch64](https://github.com/bytecodealliance/wasmtime/commit/abc000efffffffffffffffffffffffffffffffff)
* [`abc000dfffff` Revert "Bump the MSRV to 1.84"](https://github.com/bytecodealliance/wasmtime/commit/abc000dfffffffffffffffffffffffffffffffff)
* [`abc000cfffff` Cranelift: add an egraph rule for `x * 2`](https://github.com/bytecodealliance/wasmtime/commit/abc000cfffffffffffffffffffffffffffffffff)
* [`abc000bfffff` Bump wasm-tools to 1.220.0](https://github.com/bytecodealliance/wasmtime/commit/abc000bfffffffffffffffffffffffffffffffff)
* [`abc000afffff` winch: fix the stack alignment on aarch64](https://github.com/bytecodealliance/wasmtime/commit/abc000afffffffffffffffffffffffffffffffff)
* ... and 10 more

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
  -- benchmarks/bz2/benchmark.wasm benchmarks/spidermonkey/spidermonkey-json.wasm
```
