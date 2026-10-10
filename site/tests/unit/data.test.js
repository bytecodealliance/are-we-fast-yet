import { test } from "node:test";
import assert from "node:assert/strict";
import { Data, benchmarkForSlug, joinSeries, seriesPoints } from "../../js/data.js";

const index = {
  benchmarks: ["Geomean", "bz2/benchmark.wasm"],
  runs: [
    { run: 1, wasmtime: "a".repeat(40), date: "2026-10-01T00:00:00Z", "fingerprint-change": null },
    { run: 3, wasmtime: "c".repeat(40), date: "2026-10-03T00:00:00Z", "fingerprint-change": null },
    { run: 2, wasmtime: "b".repeat(40), date: "2026-10-02T00:00:00Z", "fingerprint-change": null },
  ],
};

test("joins points with the index by commit date", () => {
  // Run 4 is not in the index (e.g. excluded since), so is dropped.
  const series = joinSeries(index, [[1, 10], [2, 20], [3, 30], [4, 40]]);
  assert.deepEqual(series.xs, [1, 2, 3].map((d) => Date.parse(`2026-10-0${d}T00:00:00Z`) / 1000));
  assert.deepEqual(series.ys, [10, 20, 30]);
  assert.deepEqual(series.runs, [1, 2, 3]);
  assert.deepEqual(series.shas, ["a", "b", "c"].map((c) => c.repeat(40)));
  assert.deepEqual(joinSeries(index, undefined).xs, []);
});

test("series lookup", () => {
  const json = { series: { execution: { "virtual-cycles": [[1, 5]] } } };
  assert.deepEqual(seriesPoints(json, "execution", "virtual-cycles"), [[1, 5]]);
  assert.deepEqual(seriesPoints(json, "compilation", "virtual-cycles"), []);
  assert.deepEqual(seriesPoints(null, "execution", "virtual-cycles"), []);
});

test("slugs map back to benchmarks", () => {
  assert.equal(benchmarkForSlug(index, "bz2-benchmark"), "bz2/benchmark.wasm");
  assert.equal(benchmarkForSlug(index, "Geomean"), "Geomean");
  assert.equal(benchmarkForSlug(index, "nope"), null);
});

test("fetches by slug and caches", async () => {
  const urls = [];
  const fetchFn = async (url) => {
    urls.push(url);
    if (url.endsWith("missing.json")) return { ok: false, status: 404 };
    return { ok: true, json: async () => ({ url }) };
  };
  const data = new Data("data/", fetchFn);
  assert.deepEqual(await data.benchmark("bz2/benchmark.wasm"), { url: "data/benchmarks/bz2-benchmark.json" });
  await data.benchmark("bz2/benchmark.wasm");
  await data.run(3);
  await data.index();
  assert.deepEqual(urls, ["data/benchmarks/bz2-benchmark.json", "data/runs/00003.json", "data/index.json"]);
  await assert.rejects(data.json("missing.json"), /HTTP 404/);
  await assert.rejects(data.json("missing.json"), /HTTP 404/);
  assert.equal(urls.filter((u) => u.endsWith("missing.json")).length, 2);
});
