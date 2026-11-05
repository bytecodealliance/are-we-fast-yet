//! Markdown sections shared by the job summary and the regression issue. See
//! `docs/regressions.md`.

use crate::config::{Config, MinRelativeChange};
use crate::git::Commit;
use crate::regressions::{Change, Report, Suppressed, VIRTUAL_CYCLES};
use crate::runs::RunNumber;
use crate::slug::slug;
use std::fmt::{Result, Write as _};

/// The Wasmtime repository URL on github.
const WASMTIME_REPO: &str = "https://github.com/bytecodealliance/wasmtime";

/// Most commits listed.
const MAX_COMMITS: usize = 50;

/// The four columns the regressions and speedups tables share, after
/// `benchmark` and `phase`. The virtual cycles are what a change is filed and
/// sorted on, so they are bold.
const HEADER: &str = " **Δ virtual cycles** | virtual cycles noise | \
                      Δ instructions retired | instructions retired noise |";

/// `HEADER`'s separator row.
const RULE: &str = "----------------------|----------------------|\
                    ------------------------|----------------------------|";

/// A run's page on the site, and its metadata on github.
fn run_link(config: &Config, run: RunNumber) -> String {
    format!(
        "[run {run}]({}run.html?run={run}) ([metadata](https://github.com/{}/blob/main/benchmark-data/{run}/metadata.toml))",
        config.site.base_url, config.site.repo
    )
}

/// The two runs compared, and the thresholds a change had to clear.
pub(crate) fn runs(s: &mut String, report: &Report, config: &Config) -> Result {
    writeln!(
        s,
        "* Baseline: {}, Wasmtime `{}`",
        run_link(config, report.baseline),
        short(&report.baseline_commit)
    )?;
    writeln!(
        s,
        "* This run: {}, Wasmtime `{}`\n",
        run_link(config, report.run),
        short(&report.commit)
    )?;
    writeln!(
        s,
        "A change counts when a Mann-Whitney U test finds it significant at \
         the `{}` level and it clears its phase's minimum relative change:\n",
        report.significance_level,
    )?;
    threshold_table(s, &report.min_relative_change)
}

fn threshold_table(s: &mut String, min: &MinRelativeChange) -> Result {
    writeln!(s, "| phase | geomean | individual |")?;
    writeln!(s, "|-------|---------|------------|")?;
    for (phase, t) in min.phases() {
        writeln!(
            s,
            "| {phase} | {} | {} |",
            percent(t.geomean),
            percent(t.individual)
        )?;
    }
    writeln!(s)
}

/// Virtual-cycles changes under a `### {title} (n)` heading, or `No {title}.`
/// if there are none.
pub(crate) fn change_table(
    s: &mut String,
    config: &Config,
    title: &str,
    changes: &[Change],
) -> Result {
    if changes.is_empty() {
        writeln!(s, "### {title}\n")?;
        return writeln!(s, "No {}.\n", title.to_lowercase());
    }
    let site = &config.site.base_url;
    writeln!(s, "### {title} ({})\n", changes.len())?;
    writeln!(s, "| benchmark | phase |{HEADER}")?;
    writeln!(s, "|-----------|-------|{RULE}")?;
    for c in changes {
        writeln!(
            s,
            "| [`{}`]({site}benchmark.html?b={}&metric={VIRTUAL_CYCLES}) | {} | **{}** | {} | {} | {} |",
            c.benchmark,
            slug(&c.benchmark),
            c.phase,
            delta(c),
            noise(c),
            instructions(c),
            instructions_noise(c),
        )?;
    }
    writeln!(s)
}

/// How to read `change_table`'s columns.
pub(crate) fn notes(s: &mut String, config: &Config) -> Result {
    writeln!(
        s,
        "A positive delta is a slowdown, a negative delta is a speedup.\n\
         \n\
         `instructions retired` counts work done rather than how well it was \
         predicted. A regression whose instructions didn't move is often \
         Valgrind's branch predictors aliasing on a code layout change; see \
         [`docs/regressions.md`](https://github.com/{}/blob/main/docs/regressions.md).\n\
         \n\
         Each noise column is how far that event's own counts spread.\n",
        config.site.repo
    )
}

/// The other events that changed for the regressed benchmarks, collapsed.
pub(crate) fn other_counters(s: &mut String, changes: &[Change]) -> Result {
    if changes.is_empty() {
        return Ok(());
    }
    writeln!(s, "<details>")?;
    writeln!(
        s,
        "<summary>Other callgrind counters that changed for these benchmarks</summary>\n"
    )?;
    writeln!(s, "| benchmark | phase | counter | Δ | noise |")?;
    writeln!(s, "|-----------|-------|---------|---|-------|")?;
    for c in changes {
        writeln!(
            s,
            "| `{}` | {} | `{}` | {} | {} |",
            c.benchmark,
            c.phase,
            c.event,
            delta(c),
            noise(c),
        )?;
    }
    writeln!(s, "\n</details>\n")
}

/// Suppressions are visible only in the job summary, so each one shows the
/// numbers that justify it.
pub(crate) fn suppressed_table(s: &mut String, suppressed: &[Suppressed]) -> Result {
    if suppressed.is_empty() {
        return Ok(());
    }
    writeln!(s, "### Suppressed ({})\n", suppressed.len())?;
    writeln!(
        s,
        "Virtual cycles moved, but the delta is branch misses alone and the \
         instruction count held still. That is Valgrind's branch predictors \
         aliasing on a code layout change, not a regression; see \
         `docs/regressions.md`.\n"
    )?;
    writeln!(
        s,
        "| benchmark | phase | change | branch-miss share | instructions |"
    )?;
    writeln!(
        s,
        "|-----------|-------|--------|-------------------|--------------|"
    )?;
    for x in suppressed {
        writeln!(
            s,
            "| `{}` | {} | {} ({}) | {:.1}% | {} |",
            x.change.benchmark,
            x.change.phase,
            signed_percent(x.change.relative_change),
            interval(&x.change),
            x.branch_miss_share * 100.0,
            signed_percent(x.instructions_change),
        )?;
    }
    writeln!(s)
}

/// The Wasmtime commits in `old..new`: a compare link, then each commit from
/// `log` if there is one.
pub(crate) fn commits(s: &mut String, old: &str, new: &str, log: Option<&[Commit]>) -> Result {
    writeln!(s, "### Commits\n")?;
    writeln!(s, "{WASMTIME_REPO}/compare/{old}...{new}\n")?;
    let Some(log) = log else {
        return Ok(());
    };
    for c in log.iter().take(MAX_COMMITS) {
        writeln!(
            s,
            "* [`{}` {}]({WASMTIME_REPO}/commit/{})",
            short(&c.commit),
            escape(&c.subject),
            c.commit
        )?;
    }
    if log.len() > MAX_COMMITS {
        writeln!(s, "* ... and {} more", log.len() - MAX_COMMITS)?;
    }
    writeln!(s)
}

/// Escape a commit subject for a Markdown link's text. Brackets end the link
/// early and a backslash escapes whatever follows, so those are
/// backslash-escaped. GitHub also renders raw HTML, which would otherwise
/// swallow the type parameter in a subject like `Box<dyn Error>`.
fn escape(subject: &str) -> String {
    let mut out = String::with_capacity(subject.len());
    for c in subject.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '[' | ']' | '\\' => {
                out.push('\\');
                out.push(c);
            }
            _ => out.push(c),
        }
    }
    out
}

/// A change's ratio and its confidence interval, e.g.
/// `1.015x, 1.010x to 1.020x`.
fn interval(c: &Change) -> String {
    format!(
        "{:.3}x, {:.3}x to {:.3}x",
        c.ratio, c.ratio_low, c.ratio_high
    )
}

/// How far each run's counts spread, e.g. `0.012% -> 0.015%`. The suite
/// geomean aggregates means rather than samples, so it has no spread.
fn noise(c: &Change) -> String {
    match (c.old_noise, c.new_noise) {
        (Some(old), Some(new)) => spread(old, new),
        _ => "n/a".to_string(),
    }
}

/// Two runs' noise, e.g. `0.012% -> 0.015%`.
fn spread(old: f64, new: f64) -> String {
    format!("{old:.3}% -> {new:.3}%")
}

/// A change's `instructions-retired` delta, e.g. `+1.48%`. The suite geomean
/// has no counters of its own, and a run may not have recorded the event.
pub(crate) fn instructions(c: &Change) -> String {
    match c.instructions {
        Some(i) => signed_percent(i.change),
        None => "n/a".to_string(),
    }
}

/// How far each run's `instructions-retired` counts spread. See `noise`,
/// which is the same for the virtual cycles.
fn instructions_noise(c: &Change) -> String {
    match c.instructions {
        Some(i) => spread(i.old_noise, i.new_noise),
        None => "n/a".to_string(),
    }
}

/// A change as a signed percent with its confidence interval, e.g.
/// `+1.50% (+1.48% to +1.52%)`. Callers reporting the number a change was
/// filed on bold the whole cell.
///
/// `ratio` and its bounds are always at least 1, so a decrease reads them
/// inverted: they bound `old / new` rather than `new / old`, and inverting
/// swaps which end is which.
fn delta(c: &Change) -> String {
    let (low, high) = if c.is_increase() {
        (c.ratio_low - 1.0, c.ratio_high - 1.0)
    } else {
        (1.0 / c.ratio_high - 1.0, 1.0 / c.ratio_low - 1.0)
    };
    format!(
        "{} ({} to {})",
        signed_percent(c.relative_change),
        signed_percent(low),
        signed_percent(high),
    )
}

/// The first 12 characters of a SHA.
pub(crate) fn short(sha: &str) -> &str {
    &sha[..sha.len().min(12)]
}

fn percent(x: f64) -> String {
    format!("{}%", x * 100.0)
}

/// Get a string describing the given ratio `x` as a signed percent.
fn signed_percent(x: f64) -> String {
    format!("{:+.2}%", x * 100.0)
}
