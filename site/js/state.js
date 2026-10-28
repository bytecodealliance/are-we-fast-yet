// Page state lives in the URL query string, so every view is linkable.

export const DEFAULT_PHASE = "execution";

/** The metrics worth looking at first; the dropdown floats them to the top. */
export const FEATURED_METRICS = ["virtual-cycles", "instructions-retired"];
export const DEFAULT_METRIC = FEATURED_METRICS[0];

/**
 * Order `metrics` for a dropdown: the featured ones first, then a `null`
 * separator, then everything else alphabetically. The separator is left out
 * when either group is empty.
 */
export function metricOptions(metrics) {
  const featured = FEATURED_METRICS.filter((m) => metrics.includes(m));
  const rest = metrics.filter((m) => !featured.includes(m)).sort();
  if (featured.length === 0 || rest.length === 0) return [...featured, ...rest];
  return [...featured, null, ...rest];
}

/**
 * Parse `search` (e.g. `location.search`) into state, validated against
 * `index` (`data/index.json`). Unknown values fall back to defaults, and
 * `errors` says what was wrong.
 */
export function parseState(search, index) {
  const params = new URLSearchParams(search);
  const errors = [];
  const pick = (key, allowed, fallback) => {
    const value = params.get(key);
    if (value === null || value === "") return fallback;
    if (allowed.includes(value)) return value;
    errors.push(`unknown ${key} "${value}"`);
    return fallback;
  };

  const phases = index?.phases ?? [];
  const metrics = index?.metrics ?? [];
  const state = {
    b: params.get("b") || null,
    phase: pick("phase", phases, phases.includes(DEFAULT_PHASE) ? DEFAULT_PHASE : phases[0] ?? null),
    metric: pick("metric", metrics, metrics.includes(DEFAULT_METRIC) ? DEFAULT_METRIC : metrics[0] ?? null),
    run: null,
    rel: params.get("rel") === "1",
    from: null,
    to: null,
  };

  const run = params.get("run");
  if (run !== null) {
    const n = /^\d{1,5}$/.test(run) ? Number(run) : NaN;
    if (Number.isInteger(n) && n > 0) state.run = n;
    else errors.push(`bad run "${run}"`);
  }

  for (const key of ["from", "to"]) {
    const value = params.get(key);
    if (value === null || value === "") continue;
    const t = Date.parse(value);
    if (Number.isNaN(t)) errors.push(`bad ${key} "${value}"`);
    else state[key] = t / 1000;
  }
  if (state.from !== null && state.to !== null && state.from >= state.to) {
    errors.push("empty from..to range");
    state.from = state.to = null;
  }
  return { state, errors };
}

/**
 * Serialize `state` into a query string (with a leading `?`, or empty),
 * leaving out defaults so URLs stay short.
 */
export function serializeState(state) {
  const params = new URLSearchParams();
  if (state.b) params.set("b", state.b);
  if (state.phase && state.phase !== DEFAULT_PHASE) params.set("phase", state.phase);
  if (state.metric && state.metric !== DEFAULT_METRIC) params.set("metric", state.metric);
  if (state.run) params.set("run", String(state.run).padStart(5, "0"));
  if (state.rel) params.set("rel", "1");
  for (const key of ["from", "to"]) {
    if (state[key] !== null && state[key] !== undefined) {
      // Whole seconds keep URLs short.
      params.set(key, new Date(Math.round(state[key]) * 1000).toISOString().replace(".000Z", "Z"));
    }
  }
  const s = params.toString();
  return s ? `?${s}` : "";
}
