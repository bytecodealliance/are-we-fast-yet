//! `config.toml`: run configuration. See `docs/architecture.md`.

use anyhow::{Context, Result};
use serde::Deserialize;
use std::path::Path;

#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct Config {
    pub benchmark: Benchmark,
    pub regressions: Regressions,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct Benchmark {
    /// Suite file, relative to `sightglass/`.
    pub suite: String,
    /// Callgrind processes per benchmark.
    pub processes: u32,
    /// Passed to `sightglass-cli` as `--engine-flags-common`.
    pub engine_flags: String,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct Regressions {
    pub significance_level: f64,
    pub min_relative_change: f64,
    /// `owner/name` of the repository regression issues are filed on.
    pub issue_repo: String,
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn loads_repo_config() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let config = Config::load(&root).unwrap();
        assert_eq!(config.benchmark.suite, "benchmarks/default.suite");
        // Line continuations leave one space between flags and no newlines.
        assert!(
            config
                .benchmark
                .engine_flags
                .starts_with("--target x86_64-unknown-linux-gnu -Ccranelift-has-sse3 ")
        );
        assert!(!config.benchmark.engine_flags.contains('\n'));
        assert!(!config.benchmark.engine_flags.contains("  "));
    }

    #[test]
    fn rejects_unknown_keys() {
        let text = r#"
            [benchmark]
            suite = "s"
            processes = 1
            engine-flags = ""
            typo = 1
            [regressions]
            significance-level = 0.01
            min-relative-change = 0.01
            issue-repo = "a/b"
        "#;
        assert!(toml::from_str::<Config>(text).is_err());
    }
}
