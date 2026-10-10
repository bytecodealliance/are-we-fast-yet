// Benchmark slugs for file names and URLs. Must match `awfy::slug` in Rust;
// both are tested against `site/tests/slug-cases.json`.

/** `bz2/benchmark.wasm` becomes `bz2-benchmark`. */
export function slug(benchmark) {
  const name = benchmark.endsWith(".wasm") ? benchmark.slice(0, -5) : benchmark;
  // Iterate by code point, as Rust iterates `char`s.
  return Array.from(name, (c) =>
    /[A-Za-z0-9._-]/.test(c) ? c : c === "/" ? "-" : "_",
  ).join("");
}
