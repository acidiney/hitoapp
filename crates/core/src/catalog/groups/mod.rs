//! One card per proven song. A group never changes a recording, its sources or any personal
//! reference; it only decides which recording a list shows for the account.
mod copies;
mod proof;

pub use copies::{RecordingCopies, RecordingCopy, RecordingGroupProof, RecordingSplit};
pub(crate) use proof::group_step;
