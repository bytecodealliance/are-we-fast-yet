//! `awfy lint-data`: check every committed run, so a bad hand edit fails CI
//! rather than the next benchmark or deploy.

use crate::runs::{self, DataDir};
use anyhow::{Result, bail};
use std::path::Path;

/// Every problem in `benchmark-data/`, or none.
pub fn lint(data: &DataDir) -> Result<Vec<String>> {
    let mut problems = Vec::new();
    let runs = data.runs()?;
    for &run in &runs {
        let dir = data.run_dir(run);
        let mut names: Vec<String> = std::fs::read_dir(&dir)?
            .map(|e| Ok(e?.file_name().to_string_lossy().into_owned()))
            .collect::<Result<_>>()?;
        names.sort();
        if names != [runs::METADATA, runs::CSV] {
            problems.push(format!(
                "{run}: expected exactly {} and {}, found: {}",
                runs::METADATA,
                runs::CSV,
                names.join(", ")
            ));
        }
        if let Err(e) = data.metadata(run) {
            problems.push(format!("{run}: {e:#}"));
        }
        match data.measurements(run) {
            Ok(m) if m.is_empty() => problems.push(format!("{run}: {} has no rows", runs::CSV)),
            Ok(_) => {}
            Err(e) => problems.push(format!("{run}: {e:#}")),
        }
    }
    match data.excluded() {
        Ok(excluded) => {
            for e in excluded {
                if !runs.iter().any(|r| r.0 == e.run) {
                    problems.push(format!(
                        "{}: excludes run {}, which does not exist",
                        runs::EXCLUDED,
                        e.run
                    ));
                }
            }
        }
        Err(e) => problems.push(format!("{e:#}")),
    }
    // Run-like names that `runs()` silently skips are probably typos.
    if data.path.is_dir() {
        for entry in std::fs::read_dir(&data.path)? {
            let name = entry?.file_name().to_string_lossy().into_owned();
            let digits = name.bytes().all(|b| b.is_ascii_digit());
            if digits && !runs.iter().any(|r| r.to_string() == name) {
                problems.push(format!("{name}: not a five-digit run number"));
            }
        }
    }
    Ok(problems)
}

pub fn run(root: &Path) -> Result<()> {
    let data = DataDir::new(root);
    let problems = lint(&data)?;
    for p in &problems {
        eprintln!("error: {p}");
    }
    if !problems.is_empty() {
        bail!("{} problem(s) in {}", problems.len(), data.path.display());
    }
    log::info!("{} runs OK", data.runs()?.len());
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture_copy() -> tempfile::TempDir {
        fn copy(from: &Path, to: &Path) {
            std::fs::create_dir_all(to).unwrap();
            for e in std::fs::read_dir(from).unwrap() {
                let e = e.unwrap();
                if e.file_type().unwrap().is_dir() {
                    copy(&e.path(), &to.join(e.file_name()));
                } else {
                    std::fs::copy(e.path(), to.join(e.file_name())).unwrap();
                }
            }
        }
        let tmp = tempfile::tempdir().unwrap();
        copy(
            &Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/benchmark-data"),
            &tmp.path().join("benchmark-data"),
        );
        tmp
    }

    #[test]
    fn fixtures_are_clean() {
        let tmp = fixture_copy();
        assert_eq!(
            lint(&DataDir::new(tmp.path())).unwrap(),
            Vec::<String>::new()
        );
    }

    #[test]
    fn empty_repo_is_clean() {
        let tmp = tempfile::tempdir().unwrap();
        assert!(lint(&DataDir::new(tmp.path())).unwrap().is_empty());
    }

    #[test]
    fn reports_every_problem() {
        let tmp = fixture_copy();
        let data = DataDir::new(tmp.path());
        let p = |run: &str, file: &str| data.path.join(run).join(file);
        // A stray file, bad metadata, a corrupt CSV, a missing CSV, an empty
        // CSV, a bad exclusion, and a mistyped run directory.
        std::fs::write(p("00001", "notes.txt"), "").unwrap();
        let meta = std::fs::read_to_string(p("00002", runs::METADATA)).unwrap();
        std::fs::write(
            p("00002", runs::METADATA),
            meta.replace("schema = 1", "schema = 2"),
        )
        .unwrap();
        std::fs::write(p("00003", runs::CSV), "not gzip").unwrap();
        std::fs::remove_file(p("00005", runs::CSV)).unwrap();
        let mut empty = Vec::new();
        runs::write_csv_gz(&[], &mut empty).unwrap();
        std::fs::write(p("00006", runs::CSV), empty).unwrap();
        std::fs::write(
            data.path.join(runs::EXCLUDED),
            "[[excluded]]\nrun = 4\nreason = \"x\"\n[[excluded]]\nrun = 77\nreason = \"y\"\n",
        )
        .unwrap();
        std::fs::create_dir(data.path.join("0007")).unwrap();

        let problems = lint(&data).unwrap();
        let has = |needle: &str| problems.iter().any(|p| p.contains(needle));
        assert!(has("00001: expected exactly"), "{problems:#?}");
        assert!(
            has("00002: parsing") && has("unsupported metadata schema 2"),
            "{problems:#?}"
        );
        assert!(has("00003:") && has("is not gzipped"), "{problems:#?}");
        assert!(has("00005: expected exactly"), "{problems:#?}");
        assert!(has("00005: opening"), "{problems:#?}");
        assert!(has("00006: sightglass.csv.gz has no rows"), "{problems:#?}");
        assert!(has("excludes run 77"), "{problems:#?}");
        assert!(has("0007: not a five-digit run number"), "{problems:#?}");
        assert_eq!(problems.len(), 8, "{problems:#?}");
    }

    #[test]
    fn bad_excluded_toml() {
        let tmp = fixture_copy();
        let data = DataDir::new(tmp.path());
        std::fs::write(
            data.path.join(runs::EXCLUDED),
            "[[excluded]]\nrun = \"x\"\n",
        )
        .unwrap();
        let problems = lint(&data).unwrap();
        assert_eq!(problems.len(), 1, "{problems:#?}");
        assert!(problems[0].contains("excluded.toml"));
    }
}
