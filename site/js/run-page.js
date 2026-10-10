// run.html?run=NNNNN: metadata, raw files, and the change table.

import { Data } from "./data.js";
import { el, showError, start } from "./page.js";
import { slug } from "./slug.js";
import { parseState } from "./state.js";
import { formatChange, formatCount } from "./transform.js";

start(async () => {
  const data = new Data();
  const index = await data.index();
  const { state } = parseState(location.search, index);
  if (state.run === null) {
    showError("No run given, or it is not a run number. Use run.html?run=NNNNN.");
    return;
  }
  if (!index.runs.some((r) => r.run === state.run)) {
    showError(`Run ${state.run} does not exist or is excluded.`);
    return;
  }
  const page = await data.run(state.run);
  const run = String(page.run).padStart(5, "0");
  document.title = `Run ${run} - Are we fast yet?`;
  document.getElementById("title").textContent = `Run ${run}`;

  const repo = `https://github.com/${index.repo}`;
  const m = page.metadata;
  const link = (href, text) => el("a", { href }, text);
  const rows = [
    ["Wasmtime", link(`https://github.com/bytecodealliance/wasmtime/commit/${m.wasmtime.commit}`, el("code", {}, m.wasmtime.commit))],
    ["Commit date", m.wasmtime["commit-date"]],
    ["Started", m.started],
    ["Finished", m.finished],
    ["Previous run", page.previous ? link(`run.html?run=${String(page.previous).padStart(5, "0")}`, String(page.previous).padStart(5, "0")) : "none"],
    ["Environment change", page["fingerprint-change"] ?? "none"],
    ["Sightglass", el("code", {}, `${m.sightglass.commit} ${m.sightglass.suite}`)],
    ["are-we-fast-yet", el("code", {}, m["are-we-fast-yet"].commit)],
    ["Workflow run", m["are-we-fast-yet"]["workflow-run"] ? link(m["are-we-fast-yet"]["workflow-run"], "logs") : "none"],
    ["Valgrind", m.environment.valgrind],
    ["rustc", m.environment.rustc],
    ["uname", el("code", {}, m.environment.uname)],
    ["CPU", m.environment.cpu],
    ["Processes", String(m.config.processes)],
    ["Engine flags", el("code", {}, m.config["engine-flags"])],
    [
      "Raw data",
      el(
        "span",
        {},
        link(`${repo}/blob/main/benchmark-data/${run}/metadata.toml`, "metadata.toml"),
        ", ",
        link(`${repo}/raw/main/benchmark-data/${run}/sightglass.csv.gz`, "sightglass.csv.gz"),
      ),
    ],
  ];
  const failures = Object.entries(m.failures ?? {});
  if (failures.length) {
    rows.push(["Failed", el("ul", {}, failures.map(([w, why]) => el("li", {}, el("code", {}, w), `: ${why}`)))]);
  }
  document.getElementById("summary").append(el("dl", {}, rows.flatMap(([k, v]) => [el("dt", {}, k), el("dd", {}, v)])));

  renderTable(page.changes);
});

function renderTable(changes) {
  const box = document.getElementById("changes");
  let key = "change";
  let descending = true;
  const columns = [
    ["benchmark", "Benchmark", false],
    ["phase", "Phase", false],
    ["mean", "Virtual cycles", true],
    ["previous", "Previous", true],
    ["change", "Change", true],
  ];

  const draw = () => {
    const sorted = [...changes].sort((a, b) => {
      // Geomean always first; nulls always last.
      const g = (b.benchmark === "Geomean") - (a.benchmark === "Geomean");
      if (g) return g;
      const [x, y] = [a[key], b[key]];
      if (x === null) return y === null ? 0 : 1;
      if (y === null) return -1;
      const c = typeof x === "number" ? x - y : String(x).localeCompare(String(y));
      return descending ? -c : c;
    });
    const header = el(
      "tr",
      {},
      columns.map(([k, label, num]) =>
        el(
          "th",
          { class: num ? "num" : "", "aria-sort": k === key ? (descending ? "descending" : "ascending") : null },
          el("button", { onclick: () => ((descending = k === key ? !descending : true), (key = k), draw()) }, label + (k === key ? (descending ? " ↓" : " ↑") : "")),
        ),
      ),
    );
    const body = sorted.map((c) =>
      el(
        "tr",
        {},
        el("td", {}, el("a", { href: `benchmark.html?b=${encodeURIComponent(slug(c.benchmark))}` }, c.benchmark)),
        el("td", {}, c.phase),
        el("td", { class: "num" }, formatCount(c.mean)),
        el("td", { class: "num" }, formatCount(c.previous)),
        el("td", { class: `num ${c.change > 0 ? "worse" : c.change < 0 ? "better" : ""}` }, formatChange(c.change)),
      ),
    );
    box.replaceChildren(el("div", { class: "table-wrap" }, el("table", {}, el("thead", {}, header), el("tbody", {}, body))));
  };
  draw();
}

