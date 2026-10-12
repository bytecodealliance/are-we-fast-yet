import { defineConfig, devices } from "@playwright/test";

export default defineConfig({
  testDir: "tests/e2e",
  fullyParallel: true,
  forbidOnly: !!process.env.CI,
  reporter: process.env.CI ? "github" : "list",
  use: {
    baseURL: "http://127.0.0.1:4173/",
  },
  webServer: {
    command: "node tests/e2e/build-fixture.js && node tests/e2e/serve.js",
    url: "http://127.0.0.1:4173/data/index.json",
    reuseExistingServer: !process.env.CI,
    // Includes compiling `awfy`.
    timeout: 10 * 60 * 1000,
  },
  projects: [{ name: "chromium", use: { ...devices["Desktop Chrome"] } }],
});
