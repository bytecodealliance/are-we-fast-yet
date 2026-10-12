// Build the fixture site with `awfy site` before the server starts.

import { execFileSync } from "node:child_process";
import { fileURLToPath } from "node:url";
import { join } from "node:path";

const repo = fileURLToPath(new URL("../../../", import.meta.url));
execFileSync(
  "cargo",
  [
    "run", "-q", "-p", "awfy", "--",
    "--root", join(repo, "crates/awfy/tests/fixtures"),
    "site",
    "--site", join(repo, "site"),
    "--out", join(repo, "target/fixture-site"),
    "--now", "2026-10-10T04:30:00Z",
  ],
  { cwd: repo, stdio: "inherit" },
);
