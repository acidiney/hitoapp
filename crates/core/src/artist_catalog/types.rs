use hito_plugin_sdk::{
    ArtistDiscographyPage, ArtistSearchPage, CatalogArtistProfile, CatalogEditionTrackList,
    CatalogReleaseTypeFilter, ReleaseGroupEditionsPage,
};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum MetadataProvider {
    #[serde(rename = "builtin_musicbrainz")]
    BuiltinMusicBrainz,
    HttpPlugin {
        installation_id: String,
    },
}

#[derive(Serialize, ToSchema)]
pub struct MetadataSettings {
    pub revision: i64,
    pub enabled: bool,
    pub providers: Vec<MetadataProvider>,
    pub automatic_artist_lookup: bool,
    pub automatic_artwork: bool,
    pub automatic_service_search: bool,
}

#[derive(Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct UpdateAutomaticLookups {
    pub automatic_artist_lookup: Option<bool>,
    pub automatic_artwork: Option<bool>,
    pub automatic_service_search: Option<bool>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum ArtistMatchingState {
    Idle,
    Queued,
    Running,
}

#[derive(Serialize, ToSchema)]
pub struct ArtistMatchingStatus {
    pub enabled: bool,
    pub state: ArtistMatchingState,
    pub matched_so_far: i64,
    #[schema(required = true)]
    pub last_finished_at: Option<i64>,
    pub last_matched: i64,
}

#[derive(Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct UpdateMetadataSettings {
    pub revision: i64,
    pub enabled: bool,
    pub providers: Vec<MetadataProvider>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum ArtistCreditRole {
    RecordingArtist,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum ArtistAssociationEvidence {
    UserConfirmed,
    MatchedFileTagArtistId,
    MatchedIsrc,
    MatchedProviderUrl,
    MatchedNameDiscography,
    MatchedCreditEvidence,
    MatchedCreditCoverage,
    UserConfirmedCandidate,
    MatchedRecordingId,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum CreditEvidenceKind {
    FileTagArtistId,
    Isrc,
    ProviderUrl,
    NameDiscography,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, ToSchema)]
pub struct CreditEvidence {
    pub kind: CreditEvidenceKind,
    pub tracks: i64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, ToSchema)]
pub struct CoverageEvidence {
    pub matched_tracks: i64,
    pub probed_tracks: i64,
}

#[derive(Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct AssociateRecordingArtist {
    pub artist_id: String,
    pub source_id: Option<String>,
    pub observation_id: String,
    pub role: ArtistCreditRole,
    pub provider: MetadataProvider,
    pub provider_artist_id: String,
    pub policy_revision: i64,
}

#[derive(Serialize, ToSchema)]
pub struct ArtistAssociation {
    pub id: String,
    pub account_id: String,
    pub library_id: String,
    pub recording_id: String,
    pub artist_id: String,
    #[schema(required = true)]
    pub source_id: Option<String>,
    pub observation_id: String,
    pub role: ArtistCreditRole,
    pub provider: MetadataProvider,
    pub provider_artist_id: String,
    pub evidence: ArtistAssociationEvidence,
    #[schema(required = true)]
    pub credit_evidence: Option<CreditEvidence>,
    #[schema(required = true)]
    pub coverage_evidence: Option<CoverageEvidence>,
    pub revision: i64,
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct ArtistReviewMetadataBinding {
    pub id: String,
    pub revision: i64,
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct ArtistReviewConnection {
    pub installation_revision: i64,
    pub service_id: String,
    pub service_revision: i64,
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct ArtistReviewContext {
    pub account_id: String,
    pub recording_id: String,
    pub library_id: String,
    pub artist_id: String,
    pub credited_name: String,
    #[schema(required = true)]
    pub source_id: Option<String>,
    pub observation_id: String,
    #[schema(required = true)]
    pub artist_operation_id: Option<String>,
    pub access_grant_id: String,
    #[schema(required = true)]
    pub metadata_binding: Option<ArtistReviewMetadataBinding>,
    pub provider: MetadataProvider,
    pub policy_revision: i64,
    #[schema(required = true)]
    pub connection: Option<ArtistReviewConnection>,
}

#[derive(Serialize, ToSchema)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum ArtistCandidateSearchAvailability {
    Available {
        context: Box<ArtistReviewContext>,
        page: ArtistSearchPage,
    },
    Disabled,
    Unsupported,
    Unavailable,
    Cancelled,
}

impl ArtistCandidateSearchAvailability {
    pub(super) fn outcome(&self) -> &'static str {
        match self {
            Self::Available { .. } => "available",
            Self::Disabled => "disabled",
            Self::Unsupported => "unsupported",
            Self::Unavailable => "unavailable",
            Self::Cancelled => "cancelled",
        }
    }
}

#[derive(Serialize, ToSchema)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum ArtistCandidatePreviewAvailability {
    Available {
        context: Box<ArtistReviewContext>,
        profile: CatalogArtistProfile,
    },
    Disabled,
    Unsupported,
    Unavailable,
    Cancelled,
}

impl ArtistCandidatePreviewAvailability {
    pub(super) fn outcome(&self) -> &'static str {
        match self {
            Self::Available { .. } => "available",
            Self::Disabled => "disabled",
            Self::Unsupported => "unsupported",
            Self::Unavailable => "unavailable",
            Self::Cancelled => "cancelled",
        }
    }
}

#[derive(Serialize, ToSchema)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum ArtistCandidateReleasesAvailability {
    Available {
        context: Box<ArtistReviewContext>,
        titles: Vec<String>,
    },
    Disabled,
    Unsupported,
    Unavailable,
    Cancelled,
}

impl ArtistCandidateReleasesAvailability {
    pub(super) fn outcome(&self) -> &'static str {
        match self {
            Self::Available { .. } => "available",
            Self::Disabled => "disabled",
            Self::Unsupported => "unsupported",
            Self::Unavailable => "unavailable",
            Self::Cancelled => "cancelled",
        }
    }
}

#[derive(Serialize, ToSchema)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum ArtistProfileAvailability {
    Available {
        association_id: String,
        profile: CatalogArtistProfile,
    },
    Disabled,
    Unsupported,
    Unavailable,
    Cancelled,
}

impl ArtistProfileAvailability {
    pub(super) fn outcome(&self) -> &'static str {
        match self {
            Self::Available { .. } => "available",
            Self::Disabled => "disabled",
            Self::Unsupported => "unsupported",
            Self::Unavailable => "unavailable",
            Self::Cancelled => "cancelled",
        }
    }
}

#[derive(Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct ReadArtistDiscography {
    pub limit: u16,
    pub continuation: Option<String>,
    #[serde(default)]
    pub release_type: Option<CatalogReleaseTypeFilter>,
}

#[derive(Serialize, ToSchema)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum ArtistDiscographyAvailability {
    Available {
        association_id: String,
        page: ArtistDiscographyPage,
        continuation: Option<String>,
    },
    Disabled,
    Unsupported,
    TypeFilterUnsupported,
    Unavailable,
    Cancelled,
}

impl ArtistDiscographyAvailability {
    pub(super) fn outcome(&self) -> &'static str {
        match self {
            Self::Available { .. } => "available",
            Self::Disabled => "disabled",
            Self::Unsupported => "unsupported",
            Self::TypeFilterUnsupported => "type_filter_unsupported",
            Self::Unavailable => "unavailable",
            Self::Cancelled => "cancelled",
        }
    }
}

#[derive(Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct ReadReleaseGroupEditions {
    pub release_group_id: String,
    pub limit: u16,
    pub continuation: Option<String>,
}

#[derive(Serialize, ToSchema)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum ReleaseGroupEditionsAvailability {
    Available {
        association_id: String,
        page: ReleaseGroupEditionsPage,
        continuation: Option<String>,
    },
    Disabled,
    Unsupported,
    Unavailable,
    Cancelled,
}

impl ReleaseGroupEditionsAvailability {
    pub(super) fn outcome(&self) -> &'static str {
        match self {
            Self::Available { .. } => "available",
            Self::Disabled => "disabled",
            Self::Unsupported => "unsupported",
            Self::Unavailable => "unavailable",
            Self::Cancelled => "cancelled",
        }
    }
}

#[derive(Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct ReadEditionTrackList {
    pub release_group_id: String,
    pub edition_id: String,
}

#[derive(Serialize, ToSchema)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum EditionTrackListAvailability {
    Available {
        association_id: String,
        edition: CatalogEditionTrackList,
    },
    Disabled,
    Unsupported,
    Unavailable,
    Cancelled,
}

impl From<EditionTrackListAvailability> for super::AlbumEditionTrackListAvailability {
    fn from(status: EditionTrackListAvailability) -> Self {
        match status {
            EditionTrackListAvailability::Disabled => Self::Disabled,
            EditionTrackListAvailability::Unsupported => Self::Unsupported,
            EditionTrackListAvailability::Cancelled => Self::Cancelled,
            EditionTrackListAvailability::Unavailable
            | EditionTrackListAvailability::Available { .. } => Self::Unavailable,
        }
    }
}

impl EditionTrackListAvailability {
    pub(super) fn outcome(&self) -> &'static str {
        match self {
            Self::Available { .. } => "available",
            Self::Disabled => "disabled",
            Self::Unsupported => "unsupported",
            Self::Unavailable => "unavailable",
            Self::Cancelled => "cancelled",
        }
    }
}
