//! The environment fingerprint: what we change deliberately, as opposed to the
//! runner's host details. See `docs/data.md`.

use crate::metadata::{Metadata, RunConfig};

/// A metadata fingerprint of the benchmarking environment and tools.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Fingerprint {
    /// The sightglass commit.
    sightglass: String,
    /// The valgrind version.
    valgrind: String,
    /// The rustc version.
    rustc: String,
    /// The configuration used when running the benchmarks.
    config: RunConfig,
}

impl Fingerprint {
    /// Create the fingerprint of the given benchmark metadata.
    pub fn of(metadata: &Metadata) -> Self {
        Self {
            sightglass: metadata.sightglass.commit.clone(),
            valgrind: metadata.environment.valgrind.clone(),
            rustc: metadata.environment.rustc.clone(),
            config: metadata.config.clone(),
        }
    }

    /// Short, human-readable descriptions of what changed from `self` to
    /// `other`, such as `"valgrind 3.22.0 -> 3.24.0"`. Empty when equal.
    pub fn diff(&self, other: &Self) -> Vec<String> {
        let mut changes = Vec::new();
        if self.sightglass != other.sightglass {
            changes.push(format!(
                "sightglass {} -> {}",
                short_sha(&self.sightglass),
                short_sha(&other.sightglass)
            ));
        }
        if self.valgrind != other.valgrind {
            changes.push(versions("valgrind", &self.valgrind, &other.valgrind, |v| {
                v.strip_prefix("valgrind-")
            }));
        }
        if self.rustc != other.rustc {
            changes.push(versions("rustc", &self.rustc, &other.rustc, |v| {
                v.strip_prefix("rustc ")?.split_whitespace().next()
            }));
        }
        if self.config.processes != other.config.processes {
            changes.push(format!(
                "config.processes {} -> {}",
                self.config.processes, other.config.processes
            ));
        }
        if self.config.engine_flags != other.config.engine_flags {
            changes.push(flags_diff(
                &self.config.engine_flags,
                &other.config.engine_flags,
            ));
        }
        changes
    }
}

fn short_sha(sha: &str) -> &str {
    &sha[..sha.len().min(7)]
}

/// `"<name> <old> -> <new>"`, using the short versions `parse` extracts when it
/// can and they differ, and the full strings otherwise (e.g. two nightlies
/// with the same version number).
fn versions(name: &str, old: &str, new: &str, parse: impl Fn(&str) -> Option<&str>) -> String {
    match (parse(old), parse(new)) {
        (Some(o), Some(n)) if o != n => format!("{name} {o} -> {n}"),
        _ => format!("{name} \"{old}\" -> \"{new}\""),
    }
}

/// The flags added and removed, e.g.
/// `"config.engine-flags added -Cfoo; removed -Cbar"`. The full strings would
/// be too long to read.
fn flags_diff(old: &str, new: &str) -> String {
    let old: Vec<&str> = old.split_whitespace().collect();
    let new: Vec<&str> = new.split_whitespace().collect();
    let added: Vec<&str> = new.iter().copied().filter(|f| !old.contains(f)).collect();
    let removed: Vec<&str> = old.iter().copied().filter(|f| !new.contains(f)).collect();
    let mut parts = Vec::new();
    if !added.is_empty() {
        parts.push(format!("added {}", added.join(" ")));
    }
    if !removed.is_empty() {
        parts.push(format!("removed {}", removed.join(" ")));
    }
    if parts.is_empty() {
        // Same flags in a different order.
        parts.push("reordered".into());
    }
    format!("config.engine-flags {}", parts.join("; "))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::metadata::tests::example;

    fn diff(change: impl FnOnce(&mut Metadata)) -> Vec<String> {
        let old = example();
        let mut new = example();
        change(&mut new);
        Fingerprint::of(&old).diff(&Fingerprint::of(&new))
    }

    #[test]
    fn equal() {
        assert!(diff(|_| {}).is_empty());
        assert_eq!(Fingerprint::of(&example()), Fingerprint::of(&example()));
    }

    #[test]
    fn host_details_are_ignored() {
        assert!(
            diff(|m| {
                m.environment.uname = "other".into();
                m.environment.cpu = "Intel".into();
                m.wasmtime.commit = "other".into();
                m.started = "2027-01-01T00:00:00Z".parse().unwrap();
            })
            .is_empty()
        );
    }

    #[test]
    fn sightglass() {
        assert_eq!(
            diff(|m| m.sightglass.commit = "1234abcd".repeat(5)),
            ["sightglass e388562 -> 1234abc"]
        );
    }

    #[test]
    fn valgrind() {
        assert_eq!(
            diff(|m| m.environment.valgrind = "valgrind-3.24.0".into()),
            ["valgrind 3.22.0 -> 3.24.0"]
        );
        assert_eq!(
            diff(|m| m.environment.valgrind = "weird".into()),
            ["valgrind \"valgrind-3.22.0\" -> \"weird\""]
        );
    }

    #[test]
    fn rustc() {
        assert_eq!(
            diff(|m| m.environment.rustc = "rustc 1.91.0 (f8297e351 2025-10-28)".into()),
            ["rustc 1.90.0 -> 1.91.0"]
        );
        assert_eq!(
            diff(|m| m.environment.rustc = "rustc 1.90.0 (deadbeef0 2025-09-15)".into()),
            [
                "rustc \"rustc 1.90.0 (1159e78c4 2025-09-14)\" -> \"rustc 1.90.0 (deadbeef0 2025-09-15)\""
            ]
        );
    }

    #[test]
    fn config() {
        assert_eq!(
            diff(|m| m.config.processes = 5),
            ["config.processes 3 -> 5"]
        );
        assert_eq!(
            diff(|m| m.config.engine_flags = "--target x86_64-unknown-linux-gnu -Cfoo".into()),
            ["config.engine-flags added -Cfoo"]
        );
        assert_eq!(
            diff(|m| m.config.engine_flags = "-Cfoo".into()),
            ["config.engine-flags added -Cfoo; removed --target x86_64-unknown-linux-gnu"]
        );
    }

    #[test]
    fn several() {
        assert_eq!(
            diff(|m| {
                m.environment.valgrind = "valgrind-3.24.0".into();
                m.config.processes = 1;
            }),
            ["valgrind 3.22.0 -> 3.24.0", "config.processes 3 -> 1"]
        );
    }
}
