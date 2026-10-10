// uPlot charts: scatter plots of one series over Wasmtime commit date, with
// environment-change markers, hover details, click-through to runs, and
// drag-to-zoom kept in the URL.

import { formatChange, formatCount, formatTicks, relativeToFirstVisible } from "./transform.js";

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
          const pad = Math.max((max - min) * 0.02, 3600);
          return [min - pad, max + pad];
        },
      },
      // uPlot passes the min and max of the points in the visible x range.
      // Pad them, without uPlot's default of stretching to zero.
      y: {
        auto: true,
        range: (u, min, max) => {
          if (min === null || max === null) return [0, 1];
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
      draw: [(u) => drawMarkers(u, markers, small)],
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
  const date = new Date(series.xs[i] * 1000).toISOString().slice(0, 10);
  tooltip.textContent = `${format(ys[i])} · run ${run} · ${series.shas[i].slice(0, 12)} · ${date}`;
  tooltip.hidden = false;
  const left = u.valToPos(series.xs[i], "x");
  const top = u.valToPos(ys[i], "y");
  tooltip.style.left = `${Math.min(left + 10, u.over.clientWidth - tooltip.offsetWidth)}px`;
  tooltip.style.top = `${Math.max(top - 30, 0)}px`;
}
