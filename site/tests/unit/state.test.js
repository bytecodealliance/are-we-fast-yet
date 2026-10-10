import { test } from "node:test";
import assert from "node:assert/strict";
import { parseState, serializeState } from "../../js/state.js";

const index = {
  phases: ["compilation", "instantiation", "execution"],
  metrics: ["virtual-cycles", "instructions-retired"],
};

test("defaults", () => {
  const { state, errors } = parseState("", index);
  assert.deepEqual(errors, []);
  assert.deepEqual(state, {
    b: null,
    phase: "execution",
    metric: "virtual-cycles",
    run: null,
    rel: false,
    from: null,
    to: null,
  });
  assert.equal(serializeState(state), "");
});

test("round trip", () => {
  const search = "?b=bz2-benchmark&phase=compilation&metric=instructions-retired&run=00042&rel=1&from=2026-10-01T00%3A00%3A00Z&to=2026-10-05T12%3A00%3A00Z";
  const { state, errors } = parseState(search, index);
  assert.deepEqual(errors, []);
  assert.equal(state.b, "bz2-benchmark");
  assert.equal(state.phase, "compilation");
  assert.equal(state.metric, "instructions-retired");
  assert.equal(state.run, 42);
  assert.equal(state.rel, true);
  assert.equal(state.from, Date.parse("2026-10-01T00:00:00Z") / 1000);
  assert.equal(serializeState(state), search);
  assert.deepEqual(parseState(serializeState(state), index).state, state);
});

test("unknown values fall back to defaults", () => {
  const { state, errors } = parseState("?phase=nope&metric=nope&run=abc&from=never", index);
  assert.equal(state.phase, "execution");
  assert.equal(state.metric, "virtual-cycles");
  assert.equal(state.run, null);
  assert.equal(state.from, null);
  assert.deepEqual(errors, ['unknown phase "nope"', 'unknown metric "nope"', 'bad run "abc"', 'bad from "never"']);
});

test("empty ranges are dropped", () => {
  const { state, errors } = parseState("?from=2026-10-05T00:00:00Z&to=2026-10-01T00:00:00Z", index);
  assert.equal(state.from, null);
  assert.equal(state.to, null);
  assert.equal(errors.length, 1);
});

test("defaults come from the index when it lacks the usual ones", () => {
  const { state } = parseState("", { phases: ["compilation"], metrics: ["instructions-retired"] });
  assert.equal(state.phase, "compilation");
  assert.equal(state.metric, "instructions-retired");
});
