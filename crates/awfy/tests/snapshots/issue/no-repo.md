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

| benchmark | phase | effect size | noise |
|-----------|-------|-------------|-------|
| [`Geomean`](https://bytecodealliance.github.io/are-we-fast-yet/benchmark.html?b=Geomean&metric=virtual-cycles) | execution | 1.007x, 1.007x to 1.008x more virtual cycles | n/a |
| [`bz2/benchmark.wasm`](https://bytecodealliance.github.io/are-we-fast-yet/benchmark.html?b=bz2-benchmark&metric=virtual-cycles) | execution | 1.015x, 1.015x to 1.015x more virtual cycles | 0.020% -> 0.020% |

<details>
<summary>Other callgrind counters that changed for these benchmarks</summary>

| benchmark | phase | effect size | noise |
|-----------|-------|-------------|-------|
| `Geomean` | execution | 1.006x, 1.006x to 1.006x more instructions retired | n/a |
| `bz2/benchmark.wasm` | execution | 1.012x, 1.012x to 1.012x more instructions retired | 0.020% -> 0.020% |

</details>

### Commits

https://github.com/bytecodealliance/wasmtime/compare/2222222222222222222222222222222222222222...3333333333333333333333333333333333333333

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
