Performance regression in 222222222222..333333333333 (are-we-fast-yet run 00003)

[are-we-fast-yet](https://github.com/bytecodealliance/are-we-fast-yet) found 4 regressions in virtual cycles (callgrind) between Wasmtime `222222222222` and `333333333333`.

* Baseline: [run 00002](https://bytecodealliance.github.io/are-we-fast-yet/run.html?run=00002) ([metadata](https://github.com/bytecodealliance/are-we-fast-yet/blob/main/benchmark-data/00002/metadata.toml))
* This run: [run 00003](https://bytecodealliance.github.io/are-we-fast-yet/run.html?run=00003) ([metadata](https://github.com/bytecodealliance/are-we-fast-yet/blob/main/benchmark-data/00003/metadata.toml))
* Threshold: significant at the 0.01 level and at least 1% more

| benchmark | phase | effect size |
|-----------|-------|-------------|
| [`Geomean`](https://bytecodealliance.github.io/are-we-fast-yet/benchmark.html?b=Geomean&metric=virtual-cycles) | execution | 1.050x +/- 0.004 more virtual cycles |
| [`spidermonkey/spidermonkey-json.wasm`](https://bytecodealliance.github.io/are-we-fast-yet/benchmark.html?b=spidermonkey-spidermonkey-json&metric=virtual-cycles) | compilation | 1.100x +/- 0.004 more virtual cycles |
| [`bz2/benchmark.wasm`](https://bytecodealliance.github.io/are-we-fast-yet/benchmark.html?b=bz2-benchmark&metric=virtual-cycles) | execution | 1.015x +/- 0.004 more virtual cycles |
| [`bz2/benchmark.wasm`](https://bytecodealliance.github.io/are-we-fast-yet/benchmark.html?b=bz2-benchmark&metric=virtual-cycles) | compilation | 1.020x +/- 0.004 more virtual cycles |

<details>
<summary>Other callgrind counters that changed for these benchmarks</summary>

| benchmark | phase | effect size |
|-----------|-------|-------------|
| `bz2/benchmark.wasm` | execution | 2.000x +/- 0.004 more l1 dcache read misses |
| `bz2/benchmark.wasm` | execution | 1.111x +/- 0.004 fewer conditional branches |

</details>

### Commits

https://github.com/bytecodealliance/wasmtime/compare/2222222222222222222222222222222222222222...3333333333333333333333333333333333333333

```
abc003b Commit number 59
abc003a Commit number 58
abc0039 Commit number 57
abc0038 Commit number 56
abc0037 Commit number 55
abc0036 Commit number 54
abc0035 Commit number 53
abc0034 Commit number 52
abc0033 Commit number 51
abc0032 Commit number 50
abc0031 Commit number 49
abc0030 Commit number 48
abc002f Commit number 47
abc002e Commit number 46
abc002d Commit number 45
abc002c Commit number 44
abc002b Commit number 43
abc002a Commit number 42
abc0029 Commit number 41
abc0028 Commit number 40
abc0027 Commit number 39
abc0026 Commit number 38
abc0025 Commit number 37
abc0024 Commit number 36
abc0023 Commit number 35
abc0022 Commit number 34
abc0021 Commit number 33
abc0020 Commit number 32
abc001f Commit number 31
abc001e Commit number 30
abc001d Commit number 29
abc001c Commit number 28
abc001b Commit number 27
abc001a Commit number 26
abc0019 Commit number 25
abc0018 Commit number 24
abc0017 Commit number 23
abc0016 Commit number 22
abc0015 Commit number 21
abc0014 Commit number 20
abc0013 Commit number 19
abc0012 Commit number 18
abc0011 Commit number 17
abc0010 Commit number 16
abc000f Commit number 15
abc000e Commit number 14
abc000d Commit number 13
abc000c Commit number 12
abc000b Commit number 11
abc000a Commit number 10
... and 10 more
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
  -- benchmarks/bz2/benchmark.wasm benchmarks/spidermonkey/spidermonkey-json.wasm
```
