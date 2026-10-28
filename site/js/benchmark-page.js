// benchmark.html?b=<slug>&metric=<m>: one plot per phase.

import { Data, benchmarkForSlug, joinSeries, seriesPoints } from "./data.js";
import { el, relToggle, replaceState, select, showError, start } from "./page.js";
import { plot } from "./plot.js";
import { metricOptions, parseState } from "./state.js";
import { fingerprintMarkers } from "./transform.js";

start(async () => {
  const data = new Data();
  const index = await data.index();
  const { state, errors } = parseState(location.search, index);
  errors.forEach((e) => showError(`Ignoring ${e}.`));

  if (!state.b) {
    showError("No benchmark given. Pick one from the overview.");
    return;
  }
  const benchmark = benchmarkForSlug(index, state.b);
  if (!benchmark) {
    showError(`Unknown benchmark "${state.b}". Pick one from the overview.`);
    return;
  }
  document.title = `${benchmark} - Are we fast yet?`;
  document.getElementById("title").textContent = benchmark;

  const json = await data.benchmark(benchmark);
  const markers = fingerprintMarkers(index.runs);
  const box = document.getElementById("plots");
  let plots = [];

  const rerender = () => {
    replaceState(state);
    render();
  };
  document.getElementById("controls").append(
    select("Metric", metricOptions(index.metrics), state.metric, (v) => ((state.metric = v), rerender())),
    relToggle(state.rel, (v) => ((state.rel = v), rerender())),
  );

  function render() {
    plots.forEach((p) => p.destroy());
    plots = [];
    box.replaceChildren();
    for (const phase of index.phases) {
      const series = joinSeries(index, seriesPoints(json, phase, state.metric));
      const section = el("section", {}, el("h2", {}, phase));
      const plotBox = el("div", { class: "plot" });
      if (series.xs.length > 0) {
        section.append(
          el("p", { class: "muted" }, "Lower is better. Points are the median of each run's samples; bars span their noise."),
        );
      }
      section.append(plotBox);
      box.append(section);
      if (series.xs.length === 0) {
        plotBox.append(el("p", { class: "muted" }, "No data."));
        continue;
      }
      plots.push(
        plot(plotBox, series, {
          width: plotBox.clientWidth,
          height: 260,
          rel: state.rel,
          markers,
          from: state.from,
          to: state.to,
          onZoom: (from, to) => {
            state.from = from;
            state.to = to;
            replaceState(state);
          },
        }),
      );
    }
  }
  render();
});
