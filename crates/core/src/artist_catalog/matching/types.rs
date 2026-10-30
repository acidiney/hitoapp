use crate::ArtistIdentityGroup;
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

#[derive(Serialize, ToSchema)]
pub struct ArtistMatchSuggestions {
    pub artist_id: String,
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
