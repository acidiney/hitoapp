mod artists;
mod artwork;
mod audio;
mod availability;
mod browse;
mod candidates;
pub use candidates::PlaybackCandidates;
mod media;
mod playback;
pub use playback::PlaybackSource;
mod preferred;
mod provider;
pub use provider::{
    AdoptProviderRecording, AttachReviewedProviderSource, ProviderCandidate, ProviderSearch,
    ProviderSourceReview,
};
pub(crate) use provider::{provider_library, publish_provider_metadata};
mod releases;
mod search;
mod selection;
mod sources;
mod types;

use crate::{HostDeviceId, store::Store};
pub use artists::{Artist, ArtistDetail};
pub use artwork::{Artwork, ArtworkFetch, ArtworkFetchStatus};
pub use audio::{AudioContent, AudioRequest, AudioStream, ByteRange, OpenedAudio};
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
    pub(crate) plugins: crate::Plugins,
    pub(crate) host: HostDeviceId,
    pub(crate) artwork_slots: Arc<Semaphore>,
    pub(crate) provider_artwork_slots: Arc<Semaphore>,
    pub(crate) directories: crate::library_storage::Directories,
}
