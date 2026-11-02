use crate::{ArtistAssociationEvidence, ArtistIdentityGroup};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use uuid::Uuid;

#[derive(Clone, Deserialize, Serialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct MatchArtistsFromTags {
    pub request_id: Uuid,
    pub after: Option<String>,
    pub limit: Option<i64>,
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize, ToSchema)]
pub struct ArtistMatchReceipt {
    pub request_id: Uuid,
    pub matched: i64,
    pub skipped_ambiguous: i64,
    pub skipped_declined: i64,
    pub next_after: Option<String>,
}

#[derive(Clone, Deserialize, Serialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct MatchArtistsFromProviderEvidence {
    pub request_id: Uuid,
    pub after: Option<String>,
    pub limit: Option<i64>,
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize, ToSchema)]
pub struct ArtistProviderMatchReceipt {
    pub request_id: Uuid,
    pub matched: i64,
    pub uncertain: i64,
    pub declined: i64,
    pub unavailable: i64,
    pub next_after: Option<String>,
}

#[derive(Serialize, ToSchema)]
pub struct ArtistMatchSuggestion {
    pub group: ArtistIdentityGroup,
    pub shared_albums: i64,
    pub same_length_tracks: i64,
    pub undecided_tracks: i64,
}

#[derive(Serialize, ToSchema)]
pub struct ArtistMatchChosen {
    pub group: ArtistIdentityGroup,
    pub tracks: i64,
    pub automatic_tracks: i64,
    pub evidence: Vec<ArtistMatchEvidence>,
}

#[derive(Serialize, ToSchema)]
pub struct ArtistMatchEvidence {
    pub evidence: ArtistAssociationEvidence,
    pub tracks: i64,
}

#[derive(Serialize, ToSchema)]
pub struct ArtistMatchSuggestions {
    pub artist_id: String,
    pub chosen: Vec<ArtistMatchChosen>,
    pub undecided_tracks: i64,
    pub items: Vec<ArtistMatchSuggestion>,
    pub dismissed: i64,
    pub dismissed_items: Vec<ArtistMatchSuggestion>,
}

#[derive(Clone, Deserialize, Serialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct DismissArtistMatchSuggestion {
    pub group_id: String,
}

#[derive(Clone, Deserialize, Serialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct RestoreArtistMatchSuggestion {
    pub group_id: String,
}

#[derive(Clone, Debug, Default, Deserialize, ToSchema)]
pub struct ArtistReviewQueueQuery {
    pub offset: Option<i64>,
    pub limit: Option<i64>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum ArtistReviewOrigin {
    #[default]
    NameSearch,
    ChosenArtist,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, sqlx::FromRow, ToSchema)]
pub struct ArtistReviewCandidate {
    pub provider_artist_id: String,
    pub name: String,
    pub kind: Option<String>,
    pub disambiguation: Option<String>,
    pub country: Option<String>,
    pub corroborated_titles: i64,
    pub in_library: bool,
    #[sqlx(skip)]
    pub origin: ArtistReviewOrigin,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct ArtistReviewItem {
    pub artist_id: String,
    pub name: String,
    pub undecided_tracks: i64,
    pub needs_search: bool,
    pub search_recording_id: Option<String>,
    pub candidates: Vec<ArtistReviewCandidate>,
}

#[derive(Clone, Deserialize, Serialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct ConfirmArtistReviewCandidate {
    pub request_id: Uuid,
    pub provider_artist_id: String,
    pub after: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize, ToSchema)]
pub struct ArtistReviewConfirmation {
    pub request_id: Uuid,
    pub artist_id: String,
    pub provider_artist_id: String,
    pub applied: i64,
    pub kept: i64,
    pub remaining: i64,
    pub next_after: Option<String>,
}

#[derive(Clone, Deserialize, Serialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct DismissArtistReviewCandidates {
    pub provider_artist_id: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, ToSchema)]
pub struct ArtistReviewDismissal {
    pub artist_id: String,
    pub dismissed: i64,
}

#[derive(Clone, Deserialize, Serialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct RestoreArtistReviewCandidates {
    pub provider_artist_id: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, ToSchema)]
pub struct ArtistReviewRestoration {
    pub artist_id: String,
    pub restored: i64,
}
