# Regression detection

`awfy regressions` compares a run with its baseline: the newest earlier run not
in `excluded.toml`. It skips the comparison if the fingerprints differ or the
Wasmtime commit is unchanged.

A (benchmark, phase) regresses when its virtual cycles change is both
statistically significant and at least `min-relative-change`. Speedups appear in
the job summary but never file issues.

## Issue

The title ends in `(are-we-fast-yet run NNNNN)`, which is the idempotency key:
the workflow files nothing if an issue with that suffix exists. A regression is
reported once; later runs compare against the regressed baseline.

## Rollout

1. Dry runs: inspect the would-be issue artifact.
2. Observation: file issues on this repo for a few weeks, then set
   `min-relative-change` from the observed noise.
3. Live: set `issue-repo` to `bytecodealliance/wasmtime`.
