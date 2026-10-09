# Tokens and permissions

## `GITHUB_TOKEN`

Automatic. No setup beyond workflow `permissions:` blocks.

| Workflow | Permissions | Used for |
|----------|-------------|----------|
| `benchmark.yml` | `contents: write` | Pushing run directories to `main`. |
| `pages.yml` | `pages: write`, `id-token: write` | `actions/deploy-pages`. |
| `ci.yml` | `contents: read` | Everything else. |

During rollout, regression issues go to this repo, so `benchmark.yml` also
needs `issues: write` and uses `GITHUB_TOKEN` for them.

## GitHub App: `are-we-fast-yet`

The only new credential. Files regression issues on
`bytecodealliance/wasmtime`.

* Owned by the `bytecodealliance` org.
* Installed on `bytecodealliance/wasmtime` only.
* Repository permissions: Issues read/write. (Metadata read is implied.)
* No webhooks.

Stored in this repo:

* Variable `AWFY_APP_ID`.
* Secret `AWFY_APP_PRIVATE_KEY`.

`benchmark.yml` mints a one-hour token, scoped to the Wasmtime repo, with
`actions/create-github-app-token`, only in the issue-filing step.

An App is preferred over a personal access token because it is not tied to a
person's account, is limited to one permission on one repo, issues short-lived
tokens, and its issues are clearly attributed to a bot.

Setup needs a `bytecodealliance` org admin, once.

## Branch protection

If `main` gets a ruleset requiring PRs, `GITHUB_TOKEN` can no longer push data.
Then: install the App on this repo too, with Contents read/write, add it to the
ruleset's bypass list, and push with its token.

## Not needed

* Reading Wasmtime: public, anonymous `git` access.
* Pages hosting: covered by `GITHUB_TOKEN`.
* Any cloud storage or external service.
