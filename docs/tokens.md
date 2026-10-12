# Tokens and permissions

Workflows use `GITHUB_TOKEN` with per-workflow `permissions:`. During rollout,
issues go to this repo, so that token files them too.

## GitHub App

The only extra credential. It files issues on `bytecodealliance/wasmtime`.
Unlike a personal token, it belongs to no person, is limited to one permission
on one repo, and mints short-lived tokens.

A `bytecodealliance` admin sets it up once:

* Install on `bytecodealliance/wasmtime` only, with Issues read/write and no
  webhooks.
* Store variable `AWFY_APP_ID` and secret `AWFY_APP_PRIVATE_KEY` in this repo.

## Branch protection

If `main` requires PRs, `GITHUB_TOKEN` can't push data. Install the App here
with Contents read/write, add it to the ruleset's bypass list, and push with
its token.
