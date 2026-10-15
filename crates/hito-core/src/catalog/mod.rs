mod artists;
mod artwork;
mod availability;
mod browse;
mod media;
mod releases;
mod search;
mod sources;
mod types;

use crate::{HostDeviceId, store::Store};
pub use artists::{Artist, ArtistDetail};
pub use artwork::Artwork;
pub(crate) use availability::{SourceAccess, push_available, push_source_access};
pub use browse::Browse;
pub use media::AudioFile;
pub use releases::{Release, ReleaseDetail, ReleaseTrack};
use std::sync::Arc;
use tokio::sync::Semaphore;
pub use types::{Page, Recording, Search, Source};

#[derive(Clone)]
pub struct Catalog {
    pub(crate) store: Store,
    pub(crate) host: HostDeviceId,
    pub(crate) artwork_slots: Arc<Semaphore>,
    pub(crate) directories: crate::library_storage::Directories,
}
