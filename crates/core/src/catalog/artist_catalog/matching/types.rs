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
