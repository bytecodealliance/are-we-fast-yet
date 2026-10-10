// Fetching the generated JSON and joining it with the run index.

import { slug } from "./slug.js";

/** Fetches and caches JSON under `base` (the `data/` directory's URL). */
export class Data {
  constructor(base = "data/", fetchFn = globalThis.fetch.bind(globalThis)) {
    this.base = base;
    this.fetch = fetchFn;
    this.cache = new Map();
  }

  async json(path) {
    if (!this.cache.has(path)) {
      const promise = this.fetch(this.base + path).then((response) => {
        if (!response.ok) throw new Error(`${path}: HTTP ${response.status}`);
        return response.json();
      });
      // Forget failures, so a retry refetches.
      promise.catch(() => this.cache.delete(path));
      this.cache.set(path, promise);
    }
    return this.cache.get(path);
  }

  index() {
    return this.json("index.json");
  }

  benchmark(name) {
    return this.json(`benchmarks/${slug(name)}.json`);
  }

  run(run) {
    return this.json(`runs/${String(run).padStart(5, "0")}.json`);
  }
}

/** The benchmark in `index` whose slug is `s`, or null. */
export function benchmarkForSlug(index, s) {
  return index.benchmarks.find((b) => slug(b) === s) ?? null;
}

/**
 * Join a `[[run, value], ...]` series with `index.runs`. Returns columns for
 * uPlot (`xs` in seconds since the epoch, by Wasmtime commit date) plus the
 * run behind each point. Points for runs missing from the index (e.g.
 * excluded since) are dropped.
 */
export function joinSeries(index, points) {
  const runs = new Map(index.runs.map((r) => [r.run, r]));
  const joined = [];
  for (const [run, value] of points ?? []) {
    const r = runs.get(run);
    if (!r) continue;
    joined.push({ x: Date.parse(r.date) / 1000, y: value, run, wasmtime: r.wasmtime });
  }
  joined.sort((a, b) => a.x - b.x || a.run - b.run);
  return {
    xs: joined.map((p) => p.x),
    ys: joined.map((p) => p.y),
    runs: joined.map((p) => p.run),
    shas: joined.map((p) => p.wasmtime),
  };
}

/** The `[run, value]` points for one phase and metric, or `[]`. */
export function seriesPoints(benchmarkJson, phase, metric) {
  return benchmarkJson?.series?.[phase]?.[metric] ?? [];
}
