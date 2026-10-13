import { test } from "node:test";
import assert from "node:assert/strict";
import {
  fingerprintMarkers,
  formatChange,
  formatCount,
  formatInstant,
  formatTicks,
  relativeToFirstVisible,
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
