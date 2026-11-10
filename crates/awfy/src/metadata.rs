//! `metadata.toml`: one run's provenance. See `docs/data.md`.

use crate::git::Commit;
use anyhow::{Context, Result, bail};
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::collections::BTreeMap;
use std::fmt;
use std::path::Path;

/// The `schema` this version of `awfy` writes. It also reads schema 1, which
/// recorded one host per run, by upgrading it on load.
pub const SCHEMA: u32 = 2;

/// `metadata.toml`: recorded metadata about a benchmark run.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct Metadata {
    /// The `metadata.toml` schema version.
    pub schema: u32,
    /// When the benchmark run started.
    pub started: Timestamp,
    /// When the benchmark run finished.
    pub finished: Timestamp,
    /// The Wasmtime commit used.
    pub wasmtime: Wasmtime,
    /// The sightglass commit and benchmark suite used.
    pub sightglass: Sightglass,
    /// The version of this repo used.
    pub are_we_fast_yet: AreWeFastYet,
    /// The benchmark runner's environment and tools installed.
    pub environment: Environment,
    /// The configuration used to run the benchmarks.
    pub config: RunConfig,
    /// Wasm path to a short reason, for benchmarks with no rows in the CSV.
    #[serde(default)]
    pub failures: BTreeMap<String, String>,
}

/// Wasmtime metadata.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct Wasmtime {
    /// The Wasmtime commit.
    pub commit: String,
    /// Committer date, the x axis of the trend plots.
    pub commit_date: Timestamp,
    /// How many further commits `commits` leaves out.
    ///
    /// Declared before `commits` because TOML needs a table's values before
    /// its sub-tables.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub omitted_commits: u32,
    /// Commits new since the previous run, newest first and `commit` itself
    /// first of all. Capped at `record::MAX_COMMITS`; empty for the first run
    /// and when the log was unavailable.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub commits: Vec<Commit>,
}

fn is_zero(n: &u32) -> bool {
    *n == 0
}

/// Sightglass metadata.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct Sightglass {
    /// The sightglass commit.
    pub commit: String,
    /// The sightglass `.suite` file used (relative to the sightglass repo's
    /// root).
    pub suite: String,
}

/// Metadata about this repo.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct AreWeFastYet {
    /// The AWFY commit.
    pub commit: String,
    /// The workflow run that kicked this benchmark run off, if any.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workflow_run: Option<String>,
}

/// Metadata about the benchmark runner environment and its tools.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct Environment {
    /// The version of valgrind installed.
    pub valgrind: String,
    /// The version of `rustc` installed.
    pub rustc: String,
    /// The host of each shard, in shard order.
    pub hosts: Vec<Host>,
}

/// The host that measured one shard of a run. Excluded from the fingerprint,
/// since runners vary.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct Host {
    /// The shard's index.
    pub shard: u32,
    /// The iterations this host measured.
    pub iterations: Vec<u32>,
    /// The host's `uname -a`.
    pub uname: String,
    /// The CPU model name.
    pub cpu: String,
    /// The CPU's feature flags from `/proc/cpuinfo`. `None` for schema 1 runs,
    /// which did not record them.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cpu_flags: Option<String>,
}

/// Schema 1's `[environment]`: one host for the whole run.
#[derive(Deserialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
struct EnvironmentV1 {
    valgrind: String,
    rustc: String,
    uname: String,
    cpu: String,
}

/// A schema 1 file, which differs from schema 2 only in `[environment]`.
#[derive(Deserialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
struct MetadataV1 {
    schema: u32,
    started: Timestamp,
    finished: Timestamp,
    wasmtime: Wasmtime,
    sightglass: Sightglass,
    are_we_fast_yet: AreWeFastYet,
    environment: EnvironmentV1,
    config: RunConfig,
    #[serde(default)]
    failures: BTreeMap<String, String>,
}

impl From<MetadataV1> for Metadata {
    fn from(v1: MetadataV1) -> Self {
        let e = v1.environment;
        Self {
            schema: SCHEMA,
            started: v1.started,
            finished: v1.finished,
            wasmtime: v1.wasmtime,
            sightglass: v1.sightglass,
            are_we_fast_yet: v1.are_we_fast_yet,
            environment: Environment {
                valgrind: e.valgrind,
                rustc: e.rustc,
                hosts: vec![Host {
                    shard: 0,
                    iterations: (0..v1.config.iterations).collect(),
                    uname: e.uname,
                    cpu: e.cpu,
                    cpu_flags: None,
                }],
            },
            config: v1.config,
            failures: v1.failures,
        }
    }
}

/// The `config.toml` settings that affect measurements.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct RunConfig {
    /// The number of benchmark iterations to run.
    pub iterations: u32,
    /// The engine flags passed into sightglass.
    pub engine_flags: String,
}

impl Metadata {
    /// Read and parse the metadata file at the given path.
    pub fn load(path: &Path) -> Result<Self> {
        let text =
            std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
        Self::parse(&text).with_context(|| format!("parsing {}", path.display()))
    }

    /// Parse a metadata file from string, upgrading an older schema to the
    /// current one.
    pub fn parse(text: &str) -> Result<Self> {
        // Check the schema first, so a future schema gets a clear error rather
        // than a confusing one about its fields.
        #[derive(Deserialize)]
        struct Schema {
            schema: u32,
        }
        let Schema { schema } = toml::from_str(text).context("reading `schema`")?;
        match schema {
            1 => {
                let v1: MetadataV1 = toml::from_str(text)?;
                debug_assert_eq!(v1.schema, 1);
                Ok(v1.into())
            }
            SCHEMA => Ok(toml::from_str(text)?),
            _ => {
                bail!("unsupported metadata schema {schema}; this awfy reads schemas 1 to {SCHEMA}")
            }
        }
    }

    /// Serialize this metadata into TOML.
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
    /// Get a timestamp of the current time.
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
                omitted_commits: 0,
                commits: vec![],
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
                hosts: vec![
                    Host {
                        shard: 0,
                        iterations: vec![0, 2],
                        uname: "Linux runnervm 6.11.0-1018-azure x86_64 GNU/Linux".into(),
                        cpu: "AMD EPYC 7763 64-Core Processor".into(),
                        cpu_flags: Some("fpu vme de pse avx2".into()),
                    },
                    Host {
                        shard: 1,
                        iterations: vec![1],
                        uname: "Linux runnervm 6.11.0-1018-azure x86_64 GNU/Linux".into(),
                        cpu: "Intel(R) Xeon(R) Platinum 8370C CPU @ 2.80GHz".into(),
                        cpu_flags: Some("fpu vme de pse avx2 avx512f".into()),
                    },
                ],
            },
            config: RunConfig {
                iterations: 3,
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
            .replace("schema = 2", "schema = 3");
        let err = Metadata::parse(&text).unwrap_err();
        assert!(format!("{err:#}").contains("unsupported metadata schema 3"));
    }

    #[test]
    fn hosts_are_an_array_of_tables() {
        let text = example().to_toml().unwrap();
        assert!(
            text.contains("[[environment.hosts]]\nshard = 1\n"),
            "{text}"
        );
        assert!(text.contains("iterations = [0, 2]\n"), "{text}");
    }

    /// Schema 1 recorded one host for the whole run.
    #[test]
    fn upgrades_schema_1() {
        let text = r#"
            schema = 1
            started = 2026-10-10T03:17:00Z
            finished = 2026-10-10T04:21:12Z

            [wasmtime]
            commit = "0123456789abcdef0123456789abcdef01234567"
            commit-date = 2026-10-09T22:41:05Z

            [sightglass]
            commit = "e38856287c8871228d7ff843e29028c381a71dd7"
            suite = "benchmarks/default.suite"

            [are-we-fast-yet]
            commit = "fedcba9876543210fedcba9876543210fedcba98"

            [environment]
            valgrind = "valgrind-3.22.0"
            rustc = "rustc 1.90.0 (1159e78c4 2025-09-14)"
            uname = "Linux runnervm"
            cpu = "AMD EPYC 7763 64-Core Processor"

            [config]
            iterations = 3
            engine-flags = ""

            [failures]
        "#;
        let m = Metadata::parse(text).unwrap();
        assert_eq!(m.schema, SCHEMA);
        assert_eq!(
            m.environment.hosts,
            [Host {
                shard: 0,
                iterations: vec![0, 1, 2],
                uname: "Linux runnervm".into(),
                cpu: "AMD EPYC 7763 64-Core Processor".into(),
                cpu_flags: None,
            }]
        );
        assert_eq!(m.environment.valgrind, "valgrind-3.22.0");
        // An upgraded run writes as schema 2.
        assert_eq!(Metadata::parse(&m.to_toml().unwrap()).unwrap(), m);

        // Schema 1 with schema 2's hosts is neither.
        let mixed = text.replace("cpu = \"AMD", "hosts = []\ncpu = \"AMD");
        assert!(Metadata::parse(&mixed).is_err());
    }

    #[test]
    fn fields() {
        let m = example();
        assert_eq!(m.field("schema").unwrap(), "2");
        assert_eq!(m.field("started").unwrap(), "2026-10-10T03:17:00Z");
        assert_eq!(
            m.field("wasmtime.commit").unwrap(),
            "0123456789abcdef0123456789abcdef01234567"
        );
        assert_eq!(m.field("config.iterations").unwrap(), "3");
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
