// uPlot charts: scatter plots of one series over Wasmtime commit date, with
// environment-change markers, hover details, click-through to runs, and
// drag-to-zoom kept in the URL.

import { formatChange, formatCount, formatInstant, formatNoise, formatTicks, noiseBand, relativeToFirstVisible } from "./transform.js";

const css = (name) => getComputedStyle(document.documentElement).getPropertyValue(name).trim();

/**
 * Draw a plot into `container`.
 *
 * `series` is `joinSeries` output. `options`: `width`, `height`, `rel`
 * (relative to first visible point), `markers` (`fingerprintMarkers`),
 * `from`/`to` (x range, seconds), `onZoom(from, to)` (null, null on reset),
 * `small` (no axes labels or tooltip, for the overview grid).
 */
export function plot(container, series, options) {
  const { width, height, rel = false, markers = [], from = null, to = null, onZoom, small = false } =
    options;
  const ys = rel ? relativeToFirstVisible(series.xs, series.ys, from, to) : series.ys;
  const format = rel ? (v, digits = 3) => formatChange(v - 1, digits - 1) : formatCount;

  const tooltip = small ? null : document.createElement("div");
  if (tooltip) {
    tooltip.className = "tooltip";
    tooltip.hidden = true;
  }

  const opts = {
    width,
    height,
    legend: { show: false },
    // Timestamps are UTC, so label the axis in UTC too; otherwise a
    // viewer's local offset can show two runs from one UTC day as two
    // different days (or the reverse).
    tzDate: (ts) => uPlot.tzDate(new Date(ts * 1000), "UTC"),
    cursor: {
      drag: { x: !small, y: false, setScale: !small },
      points: { size: 8 },
    },
    scales: {
      x: {
        time: true,
        // Pad the full range so edge points are not cut in half. Zoomed
        // ranges are kept as is, so they survive a reload unchanged.
        range: (u, min, max) => {
          if (min !== series.xs[0] || max !== series.xs[series.xs.length - 1]) return [min, max];
          const pad = Math.max((max - min) * 0.02, 900);
          return [min - pad, max + pad];
        },
      },
      // uPlot passes the min and max of the points in the visible x range.
      // Pad them, without uPlot's default of stretching to zero. The error
      // bars can reach past the points, so widen to whatever they need first.
      y: {
        auto: true,
        range: (u, min, max) => {
          if (min === null || max === null) return [0, 1];
          [min, max] = withBars(u, series, ys, min, max);
          const pad = (max - min) * 0.1 || Math.abs(max) * 0.01 || 1;
          return [min - pad, max + pad];
        },
      },
    },
    axes: [
      { stroke: css("--fg-muted"), grid: { stroke: css("--grid") }, ticks: { stroke: css("--grid") }, show: !small },
      {
        stroke: css("--fg-muted"),
        grid: { stroke: css("--grid") },
        ticks: { stroke: css("--grid") },
        values: (u, vals) => formatTicks(vals, format),
        size: small ? 0 : 60,
        show: !small,
      },
    ],
    series: [
      {},
      {
        stroke: css("--accent"),
        fill: css("--accent"),
        width: 0,
        paths: () => null,
        points: { show: true, size: small ? 4 : 6, stroke: css("--accent"), fill: css("--accent") },
      },
    ],
    hooks: {
      // Bars first, so the points draw on top of their own bar.
      draw: [(u) => drawBars(u, series, ys, small), (u) => drawMarkers(u, markers, small)],
      setCursor: tooltip ? [(u) => updateTooltip(u, tooltip, series, ys, format)] : [],
      setScale: [
        (u, key) => {
          if (key !== "x" || !ready || !onZoom) return;
          const { min, max } = u.scales.x;
          const [lo, hi] = [series.xs[0], series.xs[series.xs.length - 1]];
          const full = min <= lo && max >= hi;
          onZoom(full ? null : min, full ? null : max);
        },
      ],
    },
  };

  // Ignore scale changes until the initial range is set.
  let ready = false;
  const u = new uPlot(opts, [series.xs, ys], container);
  if (from !== null && to !== null) u.setScale("x", { min: from, max: to });
  ready = true;
  if (tooltip) u.over.appendChild(tooltip);

  if (!small) {
    // Click a point to open its run.
    u.over.addEventListener("click", () => {
      const i = u.cursor.idx;
      if (i !== null && i !== undefined && series.runs[i] !== undefined && !u._awfyDragged) {
        location.href = `run.html?run=${String(series.runs[i]).padStart(5, "0")}`;
      }
    });
    u.over.addEventListener("mousedown", (e) => (u._awfyDown = e.clientX));
    u.over.addEventListener("mouseup", (e) => (u._awfyDragged = Math.abs(e.clientX - (u._awfyDown ?? e.clientX)) > 3));
    // Double click resets the zoom (uPlot's default); report it.
    u.over.addEventListener("dblclick", () => onZoom?.(null, null));
  }
  return u;
}

/**
 * Widen `[min, max]` to cover the error bars of the points in it, so a bar
 * never runs off the top or bottom of the plot.
 */
function withBars(u, series, ys, min, max) {
  for (let i = 0; i < ys.length; i++) {
    const band = noiseBand(ys[i], series.noises?.[i]);
    if (!band) continue;
    // Only points uPlot counted toward this range, i.e. the visible ones.
    if (ys[i] < min || ys[i] > max) continue;
    min = Math.min(min, band[0]);
    max = Math.max(max, band[1]);
  }
  return [min, max];
}

/**
 * Draw each point's error bar: a vertical line spanning the point's noise,
 * with a cap at each end.
 *
 * Skipped on the overview's small plots, where at 140px the bars would be a
 * smear rather than a reading.
 */
function drawBars(u, series, ys, small) {
  if (small) return;
  const ctx = u.ctx;
  const px = devicePixelRatio;
  const cap = 3 * px;
  ctx.save();
  ctx.strokeStyle = css("--accent");
  ctx.lineWidth = px;
  for (let i = 0; i < ys.length; i++) {
    const band = noiseBand(ys[i], series.noises?.[i]);
    if (!band) continue;
    const x = Math.round(u.valToPos(series.xs[i], "x", true));
    if (x < u.bbox.left || x > u.bbox.left + u.bbox.width) continue;
    const [low, high] = band.map((v) => Math.round(u.valToPos(v, "y", true)));
    // A bar shorter than its own caps reads as a blob; the point already says
    // where the value is, so draw nothing.
    if (Math.abs(low - high) < 2 * px) continue;
    ctx.beginPath();
    ctx.moveTo(x, low);
    ctx.lineTo(x, high);
    ctx.moveTo(x - cap, low);
    ctx.lineTo(x + cap, low);
    ctx.moveTo(x - cap, high);
    ctx.lineTo(x + cap, high);
    ctx.stroke();
  }
  ctx.restore();
}

function drawMarkers(u, markers, small) {
  const ctx = u.ctx;
  const px = devicePixelRatio;
  ctx.save();
  ctx.strokeStyle = css("--marker");
  ctx.fillStyle = css("--marker");
  ctx.lineWidth = px;
  ctx.font = `${11 * px}px system-ui, sans-serif`;
  ctx.textBaseline = "top";
  for (const m of markers) {
    const x = Math.round(u.valToPos(m.x, "x", true));
    if (x < u.bbox.left || x > u.bbox.left + u.bbox.width) continue;
    ctx.setLineDash([4 * px, 4 * px]);
    ctx.beginPath();
    ctx.moveTo(x, u.bbox.top);
    ctx.lineTo(x, u.bbox.top + u.bbox.height);
    ctx.stroke();
    if (!small) {
      // Label beside the line, flipped to its left near the right edge.
      const width = ctx.measureText(m.label).width;
      const right = x + 4 * px + width <= u.bbox.left + u.bbox.width;
      ctx.textAlign = right ? "left" : "right";
      ctx.fillText(m.label, right ? x + 4 * px : x - 4 * px, u.bbox.top + 2 * px);
    }
  }
  ctx.restore();
}

function updateTooltip(u, tooltip, series, ys, format) {
  const i = u.cursor.idx;
  if (i === null || i === undefined || ys[i] === null || ys[i] === undefined) {
    tooltip.hidden = true;
    return;
  }
  const run = String(series.runs[i]).padStart(5, "0");
  const when = formatInstant(series.xs[i]);
  const noise = formatNoise(series.noises?.[i]);
  const parts = [format(ys[i])];
  if (noise) parts.push(`± ${noise}`);
  parts.push(`run ${run}`, series.shas[i].slice(0, 12), when);
  tooltip.textContent = parts.join(" · ");
  tooltip.hidden = false;
  const left = u.valToPos(series.xs[i], "x");
  const top = u.valToPos(ys[i], "y");
  tooltip.style.left = `${Math.min(left + 10, u.over.clientWidth - tooltip.offsetWidth)}px`;
  tooltip.style.top = `${Math.max(top - 30, 0)}px`;
}
