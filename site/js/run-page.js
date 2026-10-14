// run.html?run=NNNNN: metadata, raw files, the new Wasmtime commits, and
// the change table.

import { Data } from "./data.js";
import { el, showError, start } from "./page.js";
import { slug } from "./slug.js";
import { parseState } from "./state.js";
import { WASMTIME_REPO, commitLog, formatChange, formatCount, shortSha } from "./transform.js";

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
  const previousSha = index.runs.find((r) => r.run === page.previous)?.wasmtime ?? null;
  const rows = [
    ["Wasmtime", link(`${WASMTIME_REPO}/commit/${m.wasmtime.commit}`, el("code", {}, m.wasmtime.commit))],
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
    ["Iterations", String(m.config.iterations)],
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

  renderCommits(commitLog(m, previousSha));
  renderTable(page.changes);
});

/** The Wasmtime commits new in this run, newest first. */
function renderCommits({ commits, omitted, compare }) {
  const box = document.getElementById("commits");
  const children = [];
  if (compare) {
    children.push(el("p", {}, el("a", { href: compare }, "Compare on GitHub")));
  }
  if (commits.length === 0) {
    children.push(
      el(
        "p",
        { class: "muted" },
        compare ? "No commit list was recorded for this run." : "No previous run to compare with.",
      ),
    );
  } else {
    children.push(
      el(
        "ul",
        { class: "commits" },
        commits.map((c) =>
          el(
            "li",
            {},
            el("a", { href: `${WASMTIME_REPO}/commit/${c.commit}` }, el("code", {}, shortSha(c.commit))),
            " ",
            c.subject,
          ),
        ),
      ),
    );
    if (omitted > 0) {
      children.push(el("p", { class: "muted" }, `... and ${omitted} more; see the comparison on GitHub.`));
    }
  }
  box.replaceChildren(...children);
}

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

