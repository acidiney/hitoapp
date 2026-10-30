mod adoption;
mod attachment;
mod bindings;
mod evidence;
mod metadata;
mod persist;
mod preparation;
mod refresh;
mod resolution;
mod search;
mod sources;

use super::Catalog;
pub use adoption::AdoptProviderRecording;
pub use attachment::{AttachReviewedProviderSource, ProviderCandidate, ProviderSourceReview};
pub(crate) use bindings::MAX_PLAYBACK_BINDINGS;
pub(crate) use metadata::publish as publish_provider_metadata;
pub(crate) use metadata::{
    ExistingProviderMetadata, refresh_existing as refresh_existing_provider_metadata,
};
pub use preparation::{
    PrepareSource, SourcePreparationPhase, SourcePreparationReceipt, SourcePreparationStatus,
};
pub(crate) use resolution::CallerAdmission;
pub(crate) use resolution::{AudioPermit, CatalogAudio, CatalogTrack};
pub use resolution::{ResolveSource, SourceResolution, SourceResolutionOutcome};
pub use search::ProviderSearch;
