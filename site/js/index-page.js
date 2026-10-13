// index.html: Geomean plot plus a grid of small per-benchmark plots.

import { Data, joinSeries, seriesPoints } from "./data.js";
import { el, relToggle, replaceState, select, showError, start } from "./page.js";
import { plot } from "./plot.js";
import { slug } from "./slug.js";
import { parseState, serializeState } from "./state.js";
import { fingerprintMarkers } from "./transform.js";

start(async () => {
  const data = new Data();
  const index = await data.index();
  const { state, errors } = parseState(location.search, index);
  errors.forEach((e) => showError(`Ignoring ${e}.`));
  const markers = fingerprintMarkers(index.runs);

  const rerender = () => {
    replaceState(state);
    render();
  };
  document.getElementById("controls").append(
    select("Phase", index.phases, state.phase, (v) => ((state.phase = v), rerender())),
    select("Metric", index.metrics, state.metric, (v) => ((state.metric = v), rerender())),
    relToggle(state.rel, (v) => ((state.rel = v), rerender())),
  );

  if (index.runs.length === 0) {
    showError("No runs yet.");
    return;
  }

  const geomeanBox = document.getElementById("geomean");
  const geomeanNote = document.getElementById("geomean-note");
  const grid = document.getElementById("grid");
  let plots = [];

  async function render() {
    plots.forEach((p) => p.destroy());
    plots = [];
    geomeanBox.replaceChildren();
    geomeanNote.hidden = true;
    grid.replaceChildren();

    const onZoom = (from, to) => {
      state.from = from;
      state.to = to;
      replaceState(state);
    };

    const [geomean, ...rest] = index.benchmarks[0] === "Geomean" ? index.benchmarks : [null, ...index.benchmarks];
    if (geomean) {
      const json = await data.benchmark(geomean);
      const series = joinSeries(index, seriesPoints(json, state.phase, state.metric));
      if (series.xs.length === 0) {
        geomeanBox.append(el("p", { class: "muted" }, "No Geomean data for this phase and metric."));
      } else {
        geomeanNote.hidden = false;
        plots.push(
          plot(geomeanBox, series, {
            width: geomeanBox.clientWidth,
            height: 320,
            rel: state.rel,
            markers,
            from: state.from,
            to: state.to,
            onZoom,
          }),
        );
      }
    }

    // Small plots load as they scroll into view.
    const observer = new IntersectionObserver(
      (entries) => {
        for (const entry of entries) {
          if (!entry.isIntersecting) continue;
          observer.unobserve(entry.target);
          const { benchmark, box } = entry.target._awfy;
          data
            .benchmark(benchmark)
            .then((json) => {
              const series = joinSeries(index, seriesPoints(json, state.phase, state.metric));
              if (series.xs.length === 0) {
                box.append(el("p", { class: "muted" }, "No data."));
                return;
              }
              plots.push(
                plot(box, series, {
                  width: box.clientWidth,
                  height: 140,
                  rel: state.rel,
                  markers,
                  from: state.from,
                  to: state.to,
                  small: true,
                }),
              );
            })
            .catch((e) => showError(`${benchmark}: ${e.message}`));
        }
      },
      { rootMargin: "200px" },
    );
    for (const benchmark of rest) {
      const box = el("div", { class: "plot" });
      const href = "benchmark.html" + serializeState({ ...state, b: slug(benchmark), rel: state.rel });
      const card = el("a", { class: "card", href }, el("h3", {}, benchmark), box);
      card._awfy = { benchmark, box };
      grid.append(card);
      observer.observe(card);
    }
  }

  render();
});
