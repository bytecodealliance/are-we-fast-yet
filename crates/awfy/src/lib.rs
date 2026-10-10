//! `awfy`: run Sightglass under callgrind, record runs, detect regressions,
//! and generate the static site. See `docs/`.

pub mod config;
pub mod fingerprint;
pub mod latest;
pub mod metadata;
pub mod record;
pub mod regressions;
pub mod run;
pub mod runs;
pub mod site;
#[cfg(test)]
mod snapshot;
