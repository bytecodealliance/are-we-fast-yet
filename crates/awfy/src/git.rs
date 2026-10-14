//! Reading a Wasmtime clone's commit log, for the commits new in a run.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// One commit: its full SHA and the first line of its message.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct Commit {
    /// The full commit SHA.
    pub commit: String,
    /// The first line of the commit message.
    pub subject: String,
}

/// `git log old..new`, so tests can fake it.
pub trait GitLog {
    /// The commits in `old..new`, newest first. `new` itself is included.
    fn log(&self, old: &str, new: &str) -> Result<Vec<Commit>>;
}

/// A local clone. A treeless one is enough: the log reads only commits.
pub struct Repo(PathBuf);

impl Repo {
    /// Create a `Repo` for the clone at `path`.
    pub fn new(path: &Path) -> Self {
        Self(path.to_path_buf())
    }
}

impl GitLog for Repo {
    fn log(&self, old: &str, new: &str) -> Result<Vec<Commit>> {
        let output = std::process::Command::new("git")
            .arg("-C")
            .arg(&self.0)
            .args([
                "log",
                // `%s` never contains a newline, so one commit is one line.
                "--format=%H %s",
                &format!("{old}..{new}"),
            ])
            .output()
            .context("running `git log`")?;
        anyhow::ensure!(
            output.status.success(),
            "`git log` in {} failed: {}",
            self.0.display(),
            String::from_utf8_lossy(&output.stderr)
        );
        parse(&String::from_utf8(output.stdout)?)
    }
}

/// Parse `--format=%H %s` output.
fn parse(output: &str) -> Result<Vec<Commit>> {
    output
        .lines()
        .map(|line| {
            let (commit, subject) = line
                .split_once(' ')
                .with_context(|| format!("malformed `git log` line `{line}`"))?;
            Ok(Commit {
                commit: commit.to_string(),
                subject: subject.to_string(),
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_log_output() {
        let log = parse("abc Fix the thing\ndef Subject  with   spaces \n").unwrap();
        assert_eq!(
            log,
            [
                Commit {
                    commit: "abc".into(),
                    subject: "Fix the thing".into()
                },
                Commit {
                    commit: "def".into(),
                    subject: "Subject  with   spaces ".into()
                },
            ]
        );
        assert_eq!(parse("").unwrap(), []);
        // An empty subject still leaves the separating space.
        assert_eq!(parse("abc ").unwrap()[0].subject, "");
        assert!(parse("abc").is_err());
    }

    #[test]
    fn logs_a_real_repo() {
        let tmp = tempfile::tempdir().unwrap();
        let git = |args: &[&str]| {
            let out = std::process::Command::new("git")
                .arg("-C")
                .arg(tmp.path())
                .args([
                    "-c",
                    "user.name=t",
                    "-c",
                    "user.email=t@t",
                    "-c",
                    "commit.gpgsign=false",
                ])
                .args(args)
                .output()
                .unwrap();
            assert!(
                out.status.success(),
                "{}",
                String::from_utf8_lossy(&out.stderr)
            );
            String::from_utf8(out.stdout).unwrap().trim().to_string()
        };
        git(&["init", "-q"]);
        let mut shas = Vec::new();
        for i in 0..4 {
            git(&[
                "commit",
                "-q",
                "--allow-empty",
                "-m",
                &format!("Commit {i}"),
            ]);
            shas.push(git(&["rev-parse", "HEAD"]));
        }
        let log = Repo::new(tmp.path()).log(&shas[0], &shas[3]).unwrap();
        let subjects: Vec<&str> = log.iter().map(|c| c.subject.as_str()).collect();
        assert_eq!(subjects, ["Commit 3", "Commit 2", "Commit 1"]);
        assert_eq!(log[0].commit, shas[3]);

        // A commit that is not in the repo is an error, not an empty log.
        assert!(
            Repo::new(tmp.path())
                .log(&"0".repeat(40), &shas[3])
                .is_err()
        );
    }
}
