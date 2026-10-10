//! `awfy latest <field>`: print a field from the newest visible run's
//! metadata, e.g. so `benchmark.yml` can skip unchanged Wasmtime commits.

use crate::metadata::Metadata;
use crate::runs::DataDir;
use anyhow::Result;
use std::path::Path;

/// The field's value, or `None` when there are no visible runs.
pub fn latest(root: &Path, field: &str) -> Result<Option<String>> {
    let data = DataDir::new(root);
    match data.visible_runs()?.last() {
        Some(&run) => Ok(Some(data.metadata(run)?.field(field)?)),
        None => {
            // Still reject unknown fields, so a typo in a workflow fails on the
            // very first run rather than later.
            Metadata::default().field(field)?;
            Ok(None)
        }
    }
}

pub fn run(root: &Path, field: &str) -> Result<()> {
    if let Some(value) = latest(root, field)? {
        println!("{value}");
    }
    Ok(())
}
