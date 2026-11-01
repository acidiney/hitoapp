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
    /// Provider answers that name no artist, several recordings or several artists.
    pub uncertain: i64,
    /// Recordings with an earlier personal decision or unlinked association.
    pub declined: i64,
    /// Recordings whose lookup could not be completed; they are asked again by a later pass.
    pub unavailable: i64,
    pub next_after: Option<String>,
}

#[derive(Serialize, ToSchema)]
pub struct ArtistMatchSuggestion {
    pub group: ArtistIdentityGroup,
    /// Distinct release titles the credit shares with the chosen artist's releases.
    pub shared_albums: i64,
    /// Tracks of the credit that match a chosen track by title and length (within three seconds).
    pub same_length_tracks: i64,
    /// Tracks of the credit that have no current artist choice and would be updated.
    pub undecided_tracks: i64,
}

/// A chosen artist that some of the credit's tracks already use.
#[derive(Serialize, ToSchema)]
pub struct ArtistMatchChosen {
    pub group: ArtistIdentityGroup,
    /// Tracks of the credit that currently use this artist.
    pub tracks: i64,
    /// Of those, the tracks Hito matched by itself.
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
    /// Chosen artists the credit's tracks already use, the most used first.
    pub chosen: Vec<ArtistMatchChosen>,
    /// Tracks of the credit with no current artist choice.
    pub undecided_tracks: i64,
    pub items: Vec<ArtistMatchSuggestion>,
    /// Suggestions the account dismissed for this credit; they are not in `items`.
    pub dismissed: i64,
    /// The dismissed suggestions that could still apply to undecided tracks, so each can be restored.
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

#[derive(Clone, Debug, PartialEq, Eq, Serialize, sqlx::FromRow, ToSchema)]
pub struct ArtistReviewCandidate {
    pub provider_artist_id: String,
    pub name: String,
    pub kind: Option<String>,
    pub disambiguation: Option<String>,
    pub country: Option<String>,
    pub corroborated_titles: i64,
    pub in_library: bool,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct ArtistReviewItem {
    pub artist_id: String,
    pub name: String,
    pub undecided_tracks: i64,
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
