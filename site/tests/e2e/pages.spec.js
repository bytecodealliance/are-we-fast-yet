import { test, expect } from "@playwright/test";
import { readFileSync } from "node:fs";

/** Fail the test on any console error or uncaught exception. */
function watchErrors(page) {
  const errors = [];
  page.on("console", (m) => m.type() === "error" && errors.push(m.text()));
  page.on("pageerror", (e) => errors.push(e.message));
  return errors;
}

test("overview renders the Geomean and every benchmark", async ({ page }) => {
  const errors = watchErrors(page);
  await page.goto("./");
  await expect(page.locator("#geomean canvas")).toHaveCount(1);
  await expect(page.locator("#grid .card")).toHaveCount(2);
  await expect(page.locator("#grid canvas")).toHaveCount(2);
  await page.screenshot({ path: "test-results/overview.png", fullPage: true });
  expect(errors).toEqual([]);
});

test("changing a selector updates the URL", async ({ page }) => {
  const errors = watchErrors(page);
  await page.goto("./");
  await expect(page.locator("#geomean canvas")).toHaveCount(1);
  await page.getByLabel("Phase").selectOption("compilation");
  await expect(page).toHaveURL(/\?phase=compilation$/);
  await page.getByLabel("Metric").selectOption("instructions-retired");
  await expect(page).toHaveURL(/phase=compilation&metric=instructions-retired/);
  await page.getByLabel("relative to first visible point").check();
  await expect(page).toHaveURL(/rel=1/);
  await expect(page.locator("#geomean canvas")).toHaveCount(1);
  expect(errors).toEqual([]);
});

test("benchmark page has a plot per phase", async ({ page }) => {
  const errors = watchErrors(page);
  await page.goto("benchmark.html?b=bz2-benchmark&metric=virtual-cycles");
  await expect(page.locator("#title")).toHaveText("bz2/benchmark.wasm");
  await expect(page.locator("#plots canvas")).toHaveCount(3);
  await page.screenshot({ path: "test-results/benchmark.png", fullPage: true });
  expect(errors).toEqual([]);
});

test("run page shows metadata and changes", async ({ page }) => {
  const errors = watchErrors(page);
  await page.goto("run.html?run=00003");
  await expect(page.locator("#title")).toHaveText("Run 00003");
  await expect(page.locator("#changes tbody tr")).toHaveCount(9);
  await expect(page.locator("#changes tbody tr").first()).toContainText("Geomean");
  await page.getByRole("button", { name: /Benchmark/ }).click();
  await page.screenshot({ path: "test-results/run.png", fullPage: true });
  expect(errors).toEqual([]);
});

test("run page lists the commits new since the previous run", async ({ page }) => {
  const errors = watchErrors(page);
  await page.goto("run.html?run=00003");
  const commits = page.locator("#commits li");
  await expect(commits).toHaveCount(3);
  await expect(commits.first()).toContainText("Speed up the thing");
  await expect(commits.first().locator("code")).toHaveText("3a3a3a3a3a3a");
  await expect(commits.first().locator("a")).toHaveAttribute(
    "href",
    `https://github.com/bytecodealliance/wasmtime/commit/${"3a".repeat(20)}`,
  );
  // A subject with quotes in it renders as text, not markup.
  await expect(commits.nth(1)).toContainText('Revert "Slow down the thing"');
  await expect(page.getByRole("link", { name: "Compare on GitHub" })).toHaveAttribute(
    "href",
    `https://github.com/bytecodealliance/wasmtime/compare/${"2".repeat(40)}...${"3".repeat(40)}`,
  );
  expect(errors).toEqual([]);
});

test("run page notes commits left out of a long log", async ({ page }) => {
  await page.goto("run.html?run=00002");
  await expect(page.locator("#commits li")).toHaveCount(1);
  await expect(page.locator("#commits")).toContainText("and 1 more");
});

test("commit subjects with markup render as text", async ({ page }) => {
  const errors = watchErrors(page);
  await page.goto("run.html?run=00005");
  const commit = page.locator("#commits li").first();
  await expect(commit).toContainText("Return Box<dyn Error> & drop the <script> hack");
  // The angle brackets are text, not an element the browser parsed.
  await expect(page.locator("#commits script")).toHaveCount(0);
  expect(errors).toEqual([]);
});

test("the first run has no commits to list", async ({ page }) => {
  await page.goto("run.html?run=00001");
  await expect(page.locator("#commits li")).toHaveCount(0);
  await expect(page.locator("#commits")).toContainText("No previous run to compare with.");
  await expect(page.getByRole("link", { name: "Compare on GitHub" })).toHaveCount(0);
});

test("bad parameters show a message", async ({ page }) => {
  for (const url of ["benchmark.html", "benchmark.html?b=nope", "run.html", "run.html?run=99999", "run.html?run=abc"]) {
    await page.goto(url);
    await expect(page.locator(".error").first()).toBeVisible();
  }
  await page.goto("./?phase=nope");
  await expect(page.locator(".error")).toContainText('unknown phase "nope"');
  await expect(page.locator("#geomean canvas")).toHaveCount(1);
});

test("issue links open a rendered benchmark page", async ({ page }) => {
  const errors = watchErrors(page);
  // The benchmark link from the regression issue snapshot, made relative.
  const body = readFileSync(new URL("../../../crates/awfy/tests/snapshots/issue/one.md", import.meta.url), "utf8");
  const match = body.match(/\]\(https:\/\/[^)]*\/(benchmark\.html\?[^)]+)\)/);
  expect(match).not.toBeNull();
  await page.goto(match[1]);
  await expect(page.locator("#plots canvas")).toHaveCount(3);
  expect(errors).toEqual([]);
});
