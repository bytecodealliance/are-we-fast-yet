import { test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { slug } from "../../js/slug.js";

test("matches the cases shared with Rust", () => {
  const cases = JSON.parse(readFileSync(new URL("../slug-cases.json", import.meta.url)));
  assert.ok(cases.length > 3);
  for (const [benchmark, want] of cases) {
    assert.equal(slug(benchmark), want, benchmark);
  }
});
