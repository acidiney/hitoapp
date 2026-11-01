mod apply;
mod coverage;
mod credit;
mod name_discography;
mod name_probe;
mod provider_evidence;
mod review_queue;
mod settle;
mod suggestions;
mod tags;
mod title;
mod types;

#[cfg(test)]
#[path = "tests/work.rs"]
mod work;

pub(crate) use provider_evidence::{
    LOOKUP_DEADLINE, LOOKUP_WINDOW, MAX_CONSECUTIVE_FAILURES, MAX_LOOKUPS, SPECIAL_PURPOSE_ARTISTS,
};
pub(crate) use title::{Title, tolerance_ms};
pub use types::*;
