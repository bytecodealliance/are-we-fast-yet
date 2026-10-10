//! `metadata.toml`: one run's provenance. See `docs/data.md`.

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::collections::BTreeMap;
use std::fmt;
use std::path::Path;

/// The only `schema` this version of `awfy` reads and writes.
pub const SCHEMA: u32 = 1;

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct Metadata {
    pub schema: u32,
    pub started: Timestamp,
    pub finished: Timestamp,
    pub wasmtime: Wasmtime,
    pub sightglass: Sightglass,
    pub are_we_fast_yet: AreWeFastYet,
    pub environment: Environment,
    pub config: RunConfig,
    /// Wasm path to a short reason, for benchmarks with no rows in the CSV.
    #[serde(default)]
    pub failures: BTreeMap<String, String>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct Wasmtime {
    pub commit: String,
    /// Committer date, the x axis of the trend plots.
    pub commit_date: Timestamp,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct Sightglass {
    pub commit: String,
    pub suite: String,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct AreWeFastYet {
    pub commit: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workflow_run: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct Environment {
    pub valgrind: String,
    pub rustc: String,
    pub uname: String,
    pub cpu: String,
}

/// The `config.toml` settings that affect measurements.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct RunConfig {
    pub processes: u32,
    pub engine_flags: String,
}

impl Metadata {
    pub fn load(path: &Path) -> Result<Self> {
        let text =
            std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
        Self::parse(&text).with_context(|| format!("parsing {}", path.display()))
    }

    pub fn parse(text: &str) -> Result<Self> {
        // Check the schema first, so a future schema gets a clear error rather
        // than a confusing one about its fields.
        #[derive(Deserialize)]
        struct Schema {
            schema: u32,
        }
        let Schema { schema } = toml::from_str(text).context("reading `schema`")?;
        if schema != SCHEMA {
            bail!("unsupported metadata schema {schema}; this awfy reads schema {SCHEMA}");
        }
        Ok(toml::from_str(text)?)
    }

    pub fn to_toml(&self) -> Result<String> {
        Ok(toml::to_string(self)?)
    }

    /// The scalar at a dotted path such as `wasmtime.commit`, as a string.
    pub fn field(&self, path: &str) -> Result<String> {
        // Round trip through text: `toml::Value::try_from` turns datetimes
        // into tables.
        let mut value = toml::Value::Table(self.to_toml()?.parse()?);
        for key in path.split('.') {
            value = match value {
                toml::Value::Table(mut t) => t.remove(key),
                _ => None,
            }
            .with_context(|| format!("unknown metadata field `{path}`"))?;
        }
        Ok(match value {
            toml::Value::String(s) => s,
            toml::Value::Table(_) | toml::Value::Array(_) => {
                bail!("metadata field `{path}` is not a single value")
            }
            other => other.to_string(),
        })
    }
}

/// A UTC instant, written as an unquoted TOML datetime such as
/// `2026-10-10T03:17:00Z`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord)]
pub struct Timestamp(pub jiff::Timestamp);

impl Timestamp {
    pub fn now() -> Self {
        // Whole seconds: sub-second precision is noise in the metadata.
        Self(jiff::Timestamp::from_second(jiff::Timestamp::now().as_second()).unwrap())
    }
}

impl fmt::Display for Timestamp {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // `jiff` displays UTC as `Z`, as RFC 3339 allows.
        self.0.fmt(f)
    }
}

impl std::str::FromStr for Timestamp {
    type Err = anyhow::Error;
    fn from_str(s: &str) -> Result<Self> {
        Ok(Self(s.parse().with_context(|| {
            format!("invalid RFC 3339 timestamp `{s}`")
        })?))
    }
}

impl Serialize for Timestamp {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        let datetime: toml::value::Datetime = self.to_string().parse().unwrap();
        datetime.serialize(s)
    }
}

impl<'de> Deserialize<'de> for Timestamp {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let datetime = toml::value::Datetime::deserialize(d)?;
        if datetime.offset.is_none() {
            return Err(serde::de::Error::custom(format!(
                "timestamp `{datetime}` needs a UTC offset"
            )));
        }
        datetime
            .to_string()
            .parse()
            .map_err(|e: anyhow::Error| serde::de::Error::custom(format!("{e:#}")))
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    pub(crate) fn example() -> Metadata {
        Metadata {
            schema: SCHEMA,
            started: "2026-10-10T03:17:00Z".parse().unwrap(),
            finished: "2026-10-10T04:21:12Z".parse().unwrap(),
            wasmtime: Wasmtime {
                commit: "0123456789abcdef0123456789abcdef01234567".into(),
                commit_date: "2026-10-09T22:41:05Z".parse().unwrap(),
            },
            sightglass: Sightglass {
                commit: "e38856287c8871228d7ff843e29028c381a71dd7".into(),
                suite: "benchmarks/default.suite".into(),
            },
            are_we_fast_yet: AreWeFastYet {
                commit: "fedcba9876543210fedcba9876543210fedcba98".into(),
                workflow_run: Some(
                    "https://github.com/bytecodealliance/are-we-fast-yet/actions/runs/123".into(),
                ),
            },
            environment: Environment {
                valgrind: "valgrind-3.22.0".into(),
                rustc: "rustc 1.90.0 (1159e78c4 2025-09-14)".into(),
                uname: "Linux runnervm 6.11.0-1018-azure x86_64 GNU/Linux".into(),
                cpu: "AMD EPYC 7763 64-Core Processor".into(),
            },
            config: RunConfig {
                processes: 3,
                engine_flags: "--target x86_64-unknown-linux-gnu".into(),
            },
            failures: BTreeMap::new(),
        }
    }

    #[test]
    fn round_trip() {
        let mut m = example();
        let text = m.to_toml().unwrap();
        assert_eq!(Metadata::parse(&text).unwrap(), m);

        m.are_we_fast_yet.workflow_run = None;
        m.failures
            .insert("bz2/benchmark.wasm".into(), "exit status 1".into());
        let text = m.to_toml().unwrap();
        assert_eq!(Metadata::parse(&text).unwrap(), m);
    }

    #[test]
    fn timestamps_are_unquoted() {
        let text = example().to_toml().unwrap();
        assert!(text.contains("started = 2026-10-10T03:17:00Z\n"), "{text}");
        assert!(
            text.contains("commit-date = 2026-10-09T22:41:05Z\n"),
            "{text}"
        );
    }

    #[test]
    fn empty_failures_are_written() {
        let text = example().to_toml().unwrap();
        assert!(text.trim_end().ends_with("[failures]"), "{text}");
    }

    #[test]
    fn rejects_other_schemas() {
        let text = example()
            .to_toml()
            .unwrap()
            .replace("schema = 1", "schema = 2");
        let err = Metadata::parse(&text).unwrap_err();
        assert!(format!("{err:#}").contains("unsupported metadata schema 2"));
    }

    #[test]
    fn fields() {
        let m = example();
        assert_eq!(m.field("schema").unwrap(), "1");
        assert_eq!(m.field("started").unwrap(), "2026-10-10T03:17:00Z");
        assert_eq!(
            m.field("wasmtime.commit").unwrap(),
            "0123456789abcdef0123456789abcdef01234567"
        );
        assert_eq!(m.field("config.processes").unwrap(), "3");
        assert!(m.field("wasmtime").is_err());
        assert!(m.field("wasmtime.nope").is_err());
        assert!(m.field("schema.nope").is_err());
    }

    #[test]
    fn rejects_local_timestamps() {
        let text = example()
            .to_toml()
            .unwrap()
            .replace("03:17:00Z", "03:17:00");
        assert!(Metadata::parse(&text).is_err());
    }
}
