//! `config.toml`: run configuration. See `docs/architecture.md`.

use anyhow::{Context, Result};
use serde::Deserialize;
use std::path::Path;

/// The struct for the top-level `config.toml` file.
#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct Config {
    /// Benchmarking config options.
    pub benchmark: Benchmark,
    /// Statistical regression config options.
    pub regressions: Regressions,
    /// Generated site config options.
    pub site: Site,
}

/// Benchmarking config options.
#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct Benchmark {
    /// Suite file, relative to `sightglass/`.
    pub suite: String,
    /// Callgrind iterations per benchmark.
    pub iterations: u32,
    /// Passed to `sightglass-cli` as `--engine-flags-common`.
    pub engine_flags: String,
}

/// Statistical regressions config options.
#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct Regressions {
    /// The significance level to test at.
    pub significance_level: f64,
    /// The minimum relative change to be considered significant.
    pub min_relative_change: f64,
    /// `owner/name` of the repository regression issues are filed on.
    issue_repo: String,
}

/// Config options for site generation.
#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct Site {
    /// Where the site is deployed, with a trailing `/`.
    pub base_url: String,
    /// `owner/name` of this repository, for links to raw data.
    pub repo: String,
}

impl Config {
    /// Load `<root>/config.toml`.
    pub fn load(root: &Path) -> Result<Self> {
        let path = root.join("config.toml");
        let text = std::fs::read_to_string(&path)
            .with_context(|| format!("reading {}", path.display()))?;
        toml::from_str(&text).with_context(|| format!("parsing {}", path.display()))
    }
}

/// The scalar at a dotted key such as `regressions.issue-repo`, as a string.
pub fn get(root: &Path, key: &str) -> Result<String> {
    // Validate the whole file first.
    Config::load(root)?;
    let text = std::fs::read_to_string(root.join("config.toml"))?;
    let mut value = toml::Value::Table(text.parse()?);
    for part in key.split('.') {
        value = match value {
            toml::Value::Table(mut t) => t.remove(part),
            _ => None,
        }
        .with_context(|| format!("unknown config key `{key}`"))?;
    }
    match value {
        toml::Value::String(s) => Ok(s),
        toml::Value::Table(_) | toml::Value::Array(_) => {
            anyhow::bail!("config key `{key}` is not a single value")
        }
        other => Ok(other.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn get_keys() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
        assert_eq!(
            get(&root, "regressions.issue-repo").unwrap(),
            "bytecodealliance/are-we-fast-yet"
        );
        assert_eq!(get(&root, "benchmark.iterations").unwrap(), "6");
        assert!(get(&root, "regressions").is_err());
        assert!(get(&root, "regressions.nope").is_err());
    }

    #[test]
    fn loads_repo_config() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let config = Config::load(&root).unwrap();
        assert_eq!(config.benchmark.suite, "benchmarks/default.suite");
        // Line continuations leave one space between flags and no newlines.
        let flags = &config.benchmark.engine_flags;
        assert!(flags.starts_with("-Cparallel-compilation=n --target x86_64-unknown-linux-gnu "));
        assert!(!flags.contains('\n'));
        assert!(!flags.contains("  "));
        assert!(config.site.base_url.ends_with('/'));
    }

    #[test]
    fn rejects_unknown_keys() {
        let text = r#"
            [benchmark]
            suite = "s"
            iterations = 1
            engine-flags = ""
            typo = 1
            [regressions]
            significance-level = 0.01
            min-relative-change = 0.01
            issue-repo = "a/b"
            [site]
            base-url = "https://example.com/"
            repo = "a/b"
        "#;
        assert!(toml::from_str::<Config>(text).is_err());
    }
}
