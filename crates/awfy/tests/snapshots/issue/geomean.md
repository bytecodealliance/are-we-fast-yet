Performance regression in 222222222222..333333333333 (are-we-fast-yet run 00003)

[are-we-fast-yet](https://github.com/bytecodealliance/are-we-fast-yet) found 1 regression in virtual cycles (callgrind) between Wasmtime `222222222222` and `333333333333`.

* Baseline: [run 00002](https://bytecodealliance.github.io/are-we-fast-yet/run.html?run=00002) ([metadata](https://github.com/bytecodealliance/are-we-fast-yet/blob/main/benchmark-data/00002/metadata.toml))
* This run: [run 00003](https://bytecodealliance.github.io/are-we-fast-yet/run.html?run=00003) ([metadata](https://github.com/bytecodealliance/are-we-fast-yet/blob/main/benchmark-data/00003/metadata.toml))
* Threshold: significant at the 0.01 level and at least 1% more

| benchmark | phase | effect size |
|-----------|-------|-------------|
| [`Geomean`](https://bytecodealliance.github.io/are-we-fast-yet/benchmark.html?b=Geomean&metric=virtual-cycles) | execution | 1.012x +/- 0.004 more virtual cycles |

### Commits

https://github.com/bytecodealliance/wasmtime/compare/2222222222222222222222222222222222222222...3333333333333333333333333333333333333333

```
abc0000 Commit number 0
```

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
  -- benchmarks/default.suite
```
