// Pure transformations of plot data.

/**
 * Divide `ys` by the first non-null y whose x is in `[from, to]` (either may
 * be null for unbounded), so the series reads as relative change. Returns
 * `ys` unchanged if there is no such point or it is zero.
 */
export function relativeToFirstVisible(xs, ys, from = null, to = null) {
  let base = null;
  for (let i = 0; i < xs.length; i++) {
    if (ys[i] === null || ys[i] === undefined) continue;
    if (from !== null && xs[i] < from) continue;
    if (to !== null && xs[i] > to) continue;
    base = ys[i];
    break;
  }
  if (!base) return ys;
  return ys.map((y) => (y === null || y === undefined ? null : y / base));
}

/**
 * Where to draw environment-change markers: between each run with a
 * `fingerprint-change` and the run before it. `runs` is `index.json`'s list.
 * Returns `[{x, label}]`, with x in seconds since the epoch.
 */
export function fingerprintMarkers(runs) {
  const markers = [];
  for (let i = 1; i < runs.length; i++) {
    const change = runs[i]["fingerprint-change"];
    if (!change) continue;
    const before = Date.parse(runs[i - 1].date) / 1000;
    const after = Date.parse(runs[i].date) / 1000;
    markers.push({ x: (before + after) / 2, label: change, run: runs[i].run });
  }
  return markers;
}

/**
 * A point's UTC timestamp, as `2026-10-01 01:41Z`. Runs can land several
 * times a day, so a bare date would label several points identically; the
 * data is all UTC, so the label says so rather than shifting to local time.
 */
export function formatInstant(seconds) {
  return new Date(seconds * 1000).toISOString().slice(0, 16).replace("T", " ") + "Z";
}

/** `1234567` as `1.23M`, with `digits` significant digits. */
export function formatCount(n, digits = 3) {
  if (n === null || n === undefined) return "";
  const abs = Math.abs(n);
  for (const [unit, size] of [["T", 1e12], ["G", 1e9], ["M", 1e6], ["k", 1e3]]) {
    if (abs >= size) return `${(n / size).toPrecision(digits)}${unit}`;
  }
  return String(Math.round(n * 100) / 100);
}

/**
 * Format axis tick values with the fewest significant digits (at least 3)
 * that keeps distinct values distinct.
 */
export function formatTicks(values, format = formatCount) {
  const distinct = new Set(values).size;
  for (let digits = 3; digits < 12; digits++) {
    const labels = values.map((v) => format(v, digits));
    if (new Set(labels).size === distinct) return labels;
  }
  return values.map((v) => format(v, 12));
}

/** `0.0123` as `+1.23%`, with `decimals` decimal places. */
export function formatChange(r, decimals = 2) {
  if (r === null || r === undefined) return "";
  const sign = r > 0 ? "+" : r < 0 ? "\u2212" : "";
  return `${sign}${Math.abs(r * 100).toFixed(decimals)}%`;
}
