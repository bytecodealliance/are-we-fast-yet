//! The `benchmark-data/` directory: run directories and `excluded.toml`. See
//! `docs/data.md`.

use crate::metadata::Metadata;
use anyhow::{Context, Result, bail};
use serde::Deserialize;
use sightglass_data::Measurement;
use std::fmt;
use std::path::{Path, PathBuf};

pub const METADATA: &str = "metadata.toml";
pub const CSV: &str = "sightglass.csv.gz";
pub const EXCLUDED: &str = "excluded.toml";

/// A run number, displayed zero-padded to five digits. Serialized as an
/// integer.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, serde::Serialize)]
#[serde(transparent)]
pub struct RunNumber(pub u32);

impl RunNumber {
    pub const MAX: u32 = 99_999;

    pub fn new(n: u32) -> Result<Self> {
        if n == 0 || n > Self::MAX {
            bail!("run number {n} is out of range 1..={}", Self::MAX);
        }
        Ok(Self(n))
    }

    /// Parse a directory name, which must be exactly five ASCII digits.
    fn from_dir_name(name: &str) -> Option<Self> {
        if name.len() == 5 && name.bytes().all(|b| b.is_ascii_digit()) {
            Self::new(name.parse().ok()?).ok()
        } else {
            None
        }
    }
}

impl fmt::Display for RunNumber {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:05}", self.0)
    }
}

impl std::str::FromStr for RunNumber {
    type Err = anyhow::Error;
    fn from_str(s: &str) -> Result<Self> {
        Self::new(
            s.parse()
                .with_context(|| format!("invalid run number `{s}`"))?,
        )
    }
}

/// `<root>/benchmark-data/`.
#[derive(Clone, Debug)]
pub struct DataDir {
    pub path: PathBuf,
}

impl DataDir {
    pub fn new(root: &Path) -> Self {
        Self {
            path: root.join("benchmark-data"),
        }
    }

    pub fn run_dir(&self, run: RunNumber) -> PathBuf {
        self.path.join(run.to_string())
    }

    /// Every run, in order. A missing directory has no runs.
    pub fn runs(&self) -> Result<Vec<RunNumber>> {
        let entries = match std::fs::read_dir(&self.path) {
            Ok(entries) => entries,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(vec![]),
            Err(e) => return Err(e).with_context(|| format!("reading {}", self.path.display())),
        };
        let mut runs = Vec::new();
        for entry in entries {
            let entry = entry?;
            if !entry.file_type()?.is_dir() {
                continue;
            }
            if let Some(run) = entry
                .file_name()
                .to_str()
                .and_then(RunNumber::from_dir_name)
            {
                runs.push(run);
            }
        }
        runs.sort();
        Ok(runs)
    }

    /// One more than the largest run, or 1 when there are none.
    pub fn next_run_number(&self) -> Result<RunNumber> {
        let last = self.runs()?.last().map_or(0, |r| r.0);
        RunNumber::new(last + 1).context("no run numbers left")
    }

    pub fn excluded(&self) -> Result<Vec<Excluded>> {
        let path = self.path.join(EXCLUDED);
        let text = match std::fs::read_to_string(&path) {
            Ok(text) => text,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(vec![]),
            Err(e) => return Err(e).with_context(|| format!("reading {}", path.display())),
        };
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct File {
            #[serde(default)]
            excluded: Vec<Excluded>,
        }
        let file: File =
            toml::from_str(&text).with_context(|| format!("parsing {}", path.display()))?;
        Ok(file.excluded)
    }

    /// Every run not in `excluded.toml`, in order.
    pub fn visible_runs(&self) -> Result<Vec<RunNumber>> {
        let excluded = self.excluded()?;
        Ok(self
            .runs()?
            .into_iter()
            .filter(|r| !excluded.iter().any(|e| e.run == r.0))
            .collect())
    }

    pub fn metadata(&self, run: RunNumber) -> Result<Metadata> {
        Metadata::load(&self.run_dir(run).join(METADATA))
    }

    pub fn measurements(&self, run: RunNumber) -> Result<Vec<Measurement<'static>>> {
        read_csv_gz(&self.run_dir(run).join(CSV))
    }
}

/// An `excluded.toml` entry.
#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Excluded {
    pub run: u32,
    pub reason: String,
}

/// Write a Sightglass raw CSV, with headers, gzipped. The output depends only
/// on `measurements`: the gzip header has no file name and a zero mtime.
pub fn write_csv_gz(measurements: &[Measurement<'_>], writer: impl std::io::Write) -> Result<()> {
    let mut gz = flate2::GzBuilder::new()
        .mtime(0)
        .write(writer, flate2::Compression::default());
    write_csv(measurements, &mut gz)?;
    gz.finish()?;
    Ok(())
}

/// Write a Sightglass raw CSV, with headers.
pub fn write_csv(measurements: &[Measurement<'_>], writer: impl std::io::Write) -> Result<()> {
    let mut csv = csv::Writer::from_writer(writer);
    for m in measurements {
        csv.serialize(m)?;
    }
    csv.flush()?;
    Ok(())
}

/// Read a gzipped Sightglass raw CSV.
pub fn read_csv_gz(path: &Path) -> Result<Vec<Measurement<'static>>> {
    let file = std::fs::File::open(path).with_context(|| format!("opening {}", path.display()))?;
    read_csv(flate2::read::GzDecoder::new(file))
        .with_context(|| format!("reading {}", path.display()))
}

/// Read a Sightglass raw CSV, with headers.
pub fn read_csv(reader: impl std::io::Read) -> Result<Vec<Measurement<'static>>> {
    // Not `sightglass_data::Format::read`, which panics on malformed rows.
    let mut reader = csv::ReaderBuilder::new()
        .has_headers(true)
        .from_reader(reader);
    let mut measurements = Vec::new();
    for row in reader.deserialize() {
        measurements.push(row?);
    }
    Ok(measurements)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn data_dir(dirs: &[&str], files: &[&str]) -> (tempfile::TempDir, DataDir) {
        let tmp = tempfile::tempdir().unwrap();
        let data = DataDir::new(tmp.path());
        std::fs::create_dir(&data.path).unwrap();
        for d in dirs {
            std::fs::create_dir(data.path.join(d)).unwrap();
        }
        for f in files {
            std::fs::write(data.path.join(f), "").unwrap();
        }
        (tmp, data)
    }

    #[test]
    fn empty() {
        let (_tmp, data) = data_dir(&[], &[]);
        assert_eq!(data.runs().unwrap(), []);
        assert_eq!(data.next_run_number().unwrap().to_string(), "00001");
    }

    #[test]
    fn missing_dir() {
        let tmp = tempfile::tempdir().unwrap();
        let data = DataDir::new(tmp.path());
        assert_eq!(data.runs().unwrap(), []);
        assert_eq!(data.excluded().unwrap(), []);
    }

    #[test]
    fn gaps_and_strays() {
        let (_tmp, data) = data_dir(
            &[
                "00003",
                "00001",
                "0002",
                "000004",
                "abcde",
                "00x05",
                ".tmp-00009",
            ],
            &["00007", "excluded.toml", ".gitkeep"],
        );
        assert_eq!(data.runs().unwrap(), [RunNumber(1), RunNumber(3)]);
        assert_eq!(data.next_run_number().unwrap(), RunNumber(4));
    }

    #[test]
    fn padding() {
        assert_eq!(RunNumber(1).to_string(), "00001");
        assert_eq!(RunNumber(12345).to_string(), "12345");
        assert_eq!("00042".parse::<RunNumber>().unwrap(), RunNumber(42));
        assert_eq!("42".parse::<RunNumber>().unwrap(), RunNumber(42));
        assert!("0".parse::<RunNumber>().is_err());
        assert!("100000".parse::<RunNumber>().is_err());
    }

    #[test]
    fn out_of_numbers() {
        let (_tmp, data) = data_dir(&["99999"], &[]);
        assert!(data.next_run_number().is_err());
    }

    #[test]
    fn exclusions() {
        let (_tmp, data) = data_dir(&["00001", "00002", "00003"], &[]);
        std::fs::write(
            data.path.join(EXCLUDED),
            "[[excluded]]\nrun = 2\nreason = \"bad\"\n",
        )
        .unwrap();
        assert_eq!(
            data.excluded().unwrap(),
            [Excluded {
                run: 2,
                reason: "bad".into()
            }]
        );
        assert_eq!(data.visible_runs().unwrap(), [RunNumber(1), RunNumber(3)]);
    }

    #[test]
    fn empty_exclusions() {
        let (_tmp, data) = data_dir(&["00001"], &[]);
        std::fs::write(data.path.join(EXCLUDED), "# nothing yet\n").unwrap();
        assert_eq!(data.excluded().unwrap(), []);
        assert_eq!(data.visible_runs().unwrap(), [RunNumber(1)]);
    }
}
