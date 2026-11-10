//! Reading a Wasmtime clone's commit log, for the commits new in a run and
//! the candidates a bisect measures.

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

/// A first-parent commit, with the parent a bisect compares it against.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct Candidate {
    /// The full commit SHA.
    pub commit: String,
    /// Its first parent's full SHA.
    pub parent: String,
    /// The first line of the commit message.
    pub subject: String,
}

/// `git log --first-parent old..new`, so tests can fake it.
pub trait FirstParents {
    /// The first-parent commits in `old..new`, newest first, each with its
    /// first parent. `new` itself is included.
    fn first_parents(&self, old: &str, new: &str) -> Result<Vec<Candidate>>;
}

impl FirstParents for Repo {
    fn first_parents(&self, old: &str, new: &str) -> Result<Vec<Candidate>> {
        let output = std::process::Command::new("git")
            .arg("-C")
            .arg(&self.0)
            .args([
                "log",
                "--first-parent",
                "--format=%H %P%x09%s",
                &format!("{old}..{new}"),
            ])
            .output()
            .context("running `git log --first-parent`")?;
        anyhow::ensure!(
            output.status.success(),
            "`git log --first-parent` in {} failed: {}",
            self.0.display(),
            String::from_utf8_lossy(&output.stderr)
        );
        parse_first_parents(&String::from_utf8(output.stdout)?)
    }
}

/// Parse `--format=%H %P%x09%s` output. `%P` lists every parent, space
/// separated; the first is the one `--first-parent` follows.
fn parse_first_parents(output: &str) -> Result<Vec<Candidate>> {
    output
        .lines()
        .map(|line| {
            let (shas, subject) = line
                .split_once('\t')
                .with_context(|| format!("malformed `git log` line `{line}`"))?;
            let mut shas = shas.split(' ');
            let commit = shas.next().unwrap_or_default();
            let parent = shas
                .next()
                .filter(|p| !p.is_empty())
                .with_context(|| format!("commit `{commit}` has no parent"))?;
            Ok(Candidate {
                commit: commit.to_string(),
                parent: parent.to_string(),
                subject: subject.to_string(),
            })
        })
        .collect()
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
    fn parses_first_parents() {
        let parsed = parse_first_parents("c p1 p2\tMerge it\nd e\tA\ttab\n").unwrap();
        assert_eq!(parsed[0].parent, "p1");
        assert_eq!(parsed[0].subject, "Merge it");
        assert_eq!(parsed[1].subject, "A\ttab");
        // A root commit has no parent to compare against.
        assert!(parse_first_parents("c \tRoot").is_err());
        assert!(parse_first_parents("c").is_err());
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

        let candidates = Repo::new(tmp.path())
            .first_parents(&shas[0], &shas[3])
            .unwrap();
        let pairs: Vec<(&str, &str, &str)> = candidates
            .iter()
            .map(|c| (c.commit.as_str(), c.parent.as_str(), c.subject.as_str()))
            .collect();
        assert_eq!(
            pairs,
            [
                (shas[3].as_str(), shas[2].as_str(), "Commit 3"),
                (shas[2].as_str(), shas[1].as_str(), "Commit 2"),
                (shas[1].as_str(), shas[0].as_str(), "Commit 1"),
            ]
        );

        // A commit that is not in the repo is an error, not an empty log.
        assert!(
            Repo::new(tmp.path())
                .log(&"0".repeat(40), &shas[3])
                .is_err()
        );
    }
}
