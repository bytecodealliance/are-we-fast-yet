import { test } from "node:test";
import assert from "node:assert/strict";
import {
  commitLog,
  fingerprintMarkers,
  formatChange,
  formatCount,
  formatInstant,
  formatNoise,
  formatTicks,
  noiseBand,
  relativeToFirstVisible,
  shortSha,
} from "../../js/transform.js";

test("relative to the first visible point", () => {
  const xs = [1, 2, 3, 4];
  const ys = [null, 200, 300, 400];
  assert.deepEqual(relativeToFirstVisible(xs, ys), [null, 1, 1.5, 2]);
  assert.deepEqual(relativeToFirstVisible(xs, ys, 3, null), [null, 200 / 300, 1, 400 / 300]);
  // Nothing visible, or a zero base: unchanged.
  assert.deepEqual(relativeToFirstVisible(xs, ys, 10, 20), ys);
  assert.deepEqual(relativeToFirstVisible([1, 2], [0, 5]), [0, 5]);
});

test("fingerprint markers sit between runs", () => {
  const runs = [
    { run: 1, date: "2026-10-01T00:00:00Z", "fingerprint-change": null },
    { run: 2, date: "2026-10-03T00:00:00Z", "fingerprint-change": "valgrind 3.22.0 -> 3.24.0" },
    { run: 3, date: "2026-10-04T00:00:00Z", "fingerprint-change": null },
  ];
  assert.deepEqual(fingerprintMarkers(runs), [
    { x: Date.parse("2026-10-02T00:00:00Z") / 1000, label: "valgrind 3.22.0 -> 3.24.0", run: 2 },
  ]);
  assert.deepEqual(fingerprintMarkers([]), []);
});

test("instants keep the time of day", () => {
  // Runs land several times a day, so two runs from one day must not get the
  // same label.
  const morning = Date.parse("2026-10-01T01:41:05Z") / 1000;
  const evening = Date.parse("2026-10-01T22:17:00Z") / 1000;
  assert.equal(formatInstant(morning), "2026-10-01 01:41Z");
  assert.equal(formatInstant(evening), "2026-10-01 22:17Z");
  assert.notEqual(formatInstant(morning), formatInstant(evening));
});

test("formatting", () => {
  assert.equal(formatCount(1234567), "1.23M");
  assert.equal(formatCount(5e9), "5.00G");
  assert.equal(formatCount(999), "999");
  assert.equal(formatCount(null), "");
  assert.equal(formatChange(0.0123), "+1.23%");
  assert.equal(formatChange(-0.03), "−3.00%");
  assert.equal(formatChange(0), "0.00%");
});

test("tick labels stay distinct", () => {
  assert.deepEqual(formatTicks([1000000, 1002000, 1004000]), ["1.000M", "1.002M", "1.004M"]);
  assert.deepEqual(formatTicks([1e6, 2e6]), ["1.00M", "2.00M"]);
  assert.deepEqual(formatTicks([]), []);
});

test("the commit log carries the new commits and a compare link", () => {
  const metadata = {
    wasmtime: {
      commit: "b".repeat(40),
      commits: [{ commit: "c".repeat(40), subject: "Speed up the thing" }],
      "omitted-commits": 2,
    },
  };
  const log = commitLog(metadata, "a".repeat(40));
  assert.deepEqual(log.commits, metadata.wasmtime.commits);
  assert.equal(log.omitted, 2);
  assert.equal(log.compare, `https://github.com/bytecodealliance/wasmtime/compare/${"a".repeat(40)}...${"b".repeat(40)}`);

  // The first run has no previous commit, so nothing to compare against.
  assert.equal(commitLog(metadata, null).compare, null);
  // A run recorded before commits were, or with no clone to read them from.
  const bare = { wasmtime: { commit: "b".repeat(40) } };
  assert.deepEqual(commitLog(bare, "a".repeat(40)).commits, []);
  assert.equal(commitLog(bare, "a".repeat(40)).omitted, 0);
});

test("commit shas are abbreviated", () => {
  assert.equal(shortSha("6a4ecd9fb47abea650e5f21f41c1806fc71af28d"), "6a4ecd9fb47a");
});

test("the noise band spans the noise, centered on the point", () => {
  // The bar's full width is the noise: 2% of 1000 is 20, so 990 to 1010.
  assert.deepEqual(noiseBand(1000, 2), [990, 1010]);
  assert.deepEqual(noiseBand(1000, 0.2), [999, 1001]);

  // Relative plots divide every point by one base, so the band scales with
  // the point and needs no adjustment.
  const [low, high] = noiseBand(2, 2);
  assert.deepEqual([low * 500, high * 500], noiseBand(1000, 2));

  // Nothing to draw without a point or without noise.
  assert.equal(noiseBand(null, 2), null);
  assert.equal(noiseBand(undefined, 2), null);
  assert.equal(noiseBand(1000, 0), null);
  assert.equal(noiseBand(1000, null), null);
  assert.equal(noiseBand(1000, undefined), null);
  assert.equal(noiseBand(1000, NaN), null);
});

test("noise is formatted with enough digits to be visible", () => {
  // Two decimals would round our quietest benchmarks to "0.00%", which is
  // exactly the distinction the figure is for.
  assert.equal(formatNoise(0.0186), "0.019%");
  assert.equal(formatNoise(7.4359), "7.436%");
  assert.equal(formatNoise(0), "0.000%");
  assert.equal(formatNoise(null), "");
  assert.equal(formatNoise(undefined), "");
});
