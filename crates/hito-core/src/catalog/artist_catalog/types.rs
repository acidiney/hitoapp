use hito_plugin_sdk::CatalogArtistProfile;
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
    pub revision: i64,
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
