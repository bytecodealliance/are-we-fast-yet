// Helpers shared by the pages.

import { serializeState } from "./state.js";

export function el(tag, attrs = {}, ...children) {
  const e = document.createElement(tag);
  for (const [k, v] of Object.entries(attrs)) {
    if (k === "class") e.className = v;
    else if (k.startsWith("on")) e.addEventListener(k.slice(2), v);
    else if (v !== null && v !== undefined && v !== false) e.setAttribute(k, v === true ? "" : v);
  }
  e.append(...children.flat().filter((c) => c !== null && c !== undefined));
  return e;
}

/** Show a visible error in `#messages`. */
export function showError(message) {
  const box = document.getElementById("messages");
  box.append(el("p", { class: "error", role: "alert" }, message));
}

/** Replace the URL's query string without a navigation. */
export function replaceState(state) {
  history.replaceState(null, "", location.pathname + serializeState(state) + location.hash);
}

/** A `<select>` over `values`, labeled. */
export function select(label, values, current, onChange) {
  const s = el(
    "select",
    { "aria-label": label, onchange: (e) => onChange(e.target.value) },
    values.map((v) => el("option", { value: v, selected: v === current }, v)),
  );
  return el("label", {}, `${label} `, s);
}

/** An absolute/relative toggle. */
export function relToggle(rel, onChange) {
  return el(
    "label",
    {},
    el("input", { type: "checkbox", checked: rel, onchange: (e) => onChange(e.target.checked) }),
    " relative to first visible point",
  );
}

/** Run `main`, turning any exception into a visible error. */
export function start(main) {
  main().catch((e) => {
    console.warn(e);
    showError(`Could not load data: ${e.message}`);
  });
}
