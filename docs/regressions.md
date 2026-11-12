# Regression detection

`awfy regressions` compares a run with its baseline: the newest earlier run not
in `excluded.toml`. It skips the comparison if the fingerprints differ or the
Wasmtime commit is unchanged.

A (benchmark, phase) regresses when its virtual cycles change is both
statistically significant and at least `min-relative-change`, and does not
carry the branch-aliasing signature below. Speedups never file issues on their
own, but are listed in a regression's issue. Suppressed regressions appear only
in the job summary.

## Thresholds

`min-relative-change` is six numbers, one per (phase, scope):

```toml
[regressions.min-relative-change.compilation]
geomean = 0.002
individual = 0.01
```

## Branch aliasing

An execution-phase regression is suppressed when branch misses account for at
least 90% of the virtual-cycles delta and `instructions-retired` moved less
than 0.1%. However, it is still listed in the job summary.

Callgrind's branch predictors are far smaller than real hardware's: the branch
target cache is 512 entries, direct-mapped and untagged, indexed by the low bits
of the branch address. A code layout change that collides two hot branches makes
them evict each other on every execution, and can cause the delta to clear the
reporting threshold while the benchmark runs the exact same instructions, from
byte-identical code, in the same wall-clock time. `awfy run` disables ASLR, so
this effect reproduces rather than averaging out.

The instruction gate keeps the rule from hiding real work: extra mispredicted
branches in real code come with extra executed branches.

## Statistics

Each (benchmark, phase, event) is compared with a Mann-Whitney U test over an
exactly enumerated null distribution, which assumes nothing about the counts'
shape; benchmark timings are right-skewed, since code cannot run faster than
its critical path but can always be interfered with. The effect size is the
median of the pairwise ratios between the two runs' samples, with a confidence
interval from inverting the same test, so the interval excludes 1 exactly when
the test finds a difference. The threshold filters on that ratio rather than on
a difference of medians, so the threshold and the test always agree about which
way a change went.

The suite geomean is compared separately, as a ratio of the two runs' geomeans
in log space. It is not a synthetic benchmark: aggregating per-sample geomeans
would sit below the figure we report by a gap that grows with a run's own
noise, making a merely noisier run read as faster.

**`iterations` and `significance-level` are coupled.** With `n` samples of each
run there are only `C(2n, n)` ways for them to interleave, so no comparison can
report a p-value below `2 / C(2n, n)`, however large the regression: 0.1 at 3
iterations, 0.0079 at 5, and 0.0022 at 6. An `iterations` too low for the
`significance-level` in `config.toml` reports nothing at all rather than
reporting less; the two are currently 6 and 0.01.

Each run's noise is also reported: how far its counts spread, as a percentage
of the median. See [site.md](site.md).

## Bisect

A run's comparison spans every commit since its baseline, and on a noisy pair
of runners it can be wrong. So a run with regressions files nothing itself:
it starts `bisect.yml`, which measures each first-parent commit in the range
against its own parent, all at once, one job per commit, and files only what
that confirms.

* **Plan.** `awfy bisect-plan` lists the candidates and the (benchmark,
  phase) pairs to measure: each regressed benchmark in the phase it regressed,
  and for a regressed Geomean, every benchmark the run measured, in that phase
  only. `awfy run --pair` passes Sightglass `--benchmark-phase`, so the other
  phases are never measured.
* **Single commit.** A range of one commit has nothing to bisect: that
  commit made the whole change. The plan's matrix is empty, nothing is
  measured, and the report blames the commit outright, so the main run's
  detection is filed unchecked.
* **Measure.** Each job builds the candidate and its parent and measures both
  on the same runner, at `[bisect] iterations` each, so host effects cancel.
  Two iterations suffice: the main run already found the regression
  significant, and the bisect only has to tell "no change" from it.
* **Attribute.** `awfy bisect-report` takes each candidate's own ratio,
  candidate over parent, for each regressed pair, recomputing a Geomean the
  way the main run does. In log terms, with `d` the detected change:
  * a candidate is **blamed** when its change is at least `blame-share` of
    `d`;
  * the pair is **complete** when the candidates' changes sum to within
    `completeness-tolerance` of `d`;
  * the pair is **confirmed** when any candidate is blamed, or the changes sum
    to within that tolerance of `d` (creep across several commits).

| Outcome | Action |
|---|---|
| The range is a single commit | File the issue, naming that commit |
| A pair is confirmed | File the issue, naming the blamed commits and each one's share |
| Every candidate was measured and no pair is confirmed | File nothing; the job summary reports an unconfirmed detection |
| No plan, nothing measured, or unmeasured candidates and no confirmation | File the range issue, so a real regression is never lost |

The thresholds are starting points to tune against real regressions.

## Issue

`bisect.yml` files it. The title names the blamed commit and its PR, or how
many commits were blamed, and ends in `(are-we-fast-yet run NNNNN)`, which is
the idempotency key: the workflow files nothing if an issue with that suffix
exists. A regression is reported once; later runs compare against the
regressed baseline. The body opens with the bisect's attribution and links its
workflow run, and its reproduce section builds the blamed commit and its
parent when one commit is blamed.

The issue body and the job summary share their run links, regressions and
speedups tables, and Wasmtime commit links. Each table reports each change as a signed percent with its confidence
interval, in four columns: the virtual cycles a change is filed on, then
`instructions-retired`, each followed by that event's own noise.

Instructions separate the engine doing more work from Valgrind mispredicting
the same work: a regression whose instructions barely moved is usually the
branch aliasing described above, caught below the 90% share the suppression
rule requires. Both instructions columns read `n/a` for the suite geomean,
which has no counters of its own, and for a run that did not record the event.

## Rollout

1. Dry runs: inspect the would-be issue artifact.
2. Observation: file issues on this repo for a few weeks, then set each
   `min-relative-change` from the observed noise.
3. Live: set `issue-repo` to `bytecodealliance/wasmtime`.
