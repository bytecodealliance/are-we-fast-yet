//! `awfy`: run Sightglass under callgrind, record runs, detect regressions,
//! and generate the static site. See `docs/`.

#![deny(missing_docs)]

pub mod bisect;
pub mod config;
mod fingerprint;
pub mod git;
pub mod issue;
pub mod latest;
pub mod lint;
mod markdown;
pub mod metadata;
pub mod record;
pub mod regressions;
pub mod run;
pub mod runs;
pub mod site;
mod slug;
#[cfg(test)]
mod snapshot;
