//! `awfy`: run Sightglass under callgrind, record runs, detect regressions,
//! and generate the static site. See `docs/`.

#![deny(missing_docs)]

pub mod config;
pub mod fingerprint;
pub mod issue;
pub mod latest;
pub mod lint;
pub mod metadata;
pub mod record;
pub mod regressions;
pub mod run;
pub mod runs;
pub mod site;
pub mod slug;
#[cfg(test)]
mod snapshot;
