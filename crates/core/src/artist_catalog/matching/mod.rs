mod apply;
mod name_discography;
mod provider_evidence;
mod settle;
mod suggestions;
mod tags;
mod types;

#[cfg(test)]
#[path = "tests/work.rs"]
mod work;

pub(crate) use provider_evidence::{
    LOOKUP_DEADLINE, LOOKUP_WINDOW, MAX_CONSECUTIVE_FAILURES, MAX_LOOKUPS, SPECIAL_PURPOSE_ARTISTS,
};
pub use types::*;
