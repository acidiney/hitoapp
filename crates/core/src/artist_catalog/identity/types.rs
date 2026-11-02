use crate::{ArtistReviewContext, MetadataProvider, Page, Recording, Release};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use uuid::Uuid;

#[derive(Deserialize, Serialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct ChooseRecordingArtistIdentity {
    pub request_id: Uuid,
    pub expected_revision: i64,
    pub association_id: Uuid,
    pub context: ArtistReviewContext,
}

#[derive(Deserialize, Serialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct ResetRecordingArtistIdentity {
    pub request_id: Uuid,
    pub expected_revision: i64,
}

#[derive(Deserialize, Serialize, ToSchema)]
pub struct ArtistIdentitySelection {
    pub group_id: String,
    pub association_id: String,
}

#[derive(Serialize, ToSchema)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum RecordingArtistIdentityState {
    Unselected,
    Selected(ArtistIdentitySelection),
    NeedsReview(ArtistIdentitySelection),
}

#[derive(Serialize, ToSchema)]
pub struct RecordingArtistIdentityChoice {
    pub recording_id: String,
    pub revision: i64,
    pub state: RecordingArtistIdentityState,
}

#[derive(Deserialize, Serialize, ToSchema)]
pub struct ArtistIdentityChoiceReceipt {
    pub request_id: Uuid,
    pub recording_id: String,
    pub revision: i64,
    pub selection: Option<ArtistIdentitySelection>,
}

#[derive(Serialize, ToSchema)]
pub struct ArtistIdentityGroupName {
    pub value: String,
    pub recording_id: String,
    pub artist_id: String,
    pub association_id: String,
}

#[derive(Serialize, ToSchema)]
pub struct ArtistIdentityGroup {
    pub id: String,
    pub provider: MetadataProvider,
    pub provider_artist_id: String,
    pub name: ArtistIdentityGroupName,
    pub library_count: i64,
    pub recording_count: i64,
    pub release_count: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub credited_photo_artist_id: Option<String>,
}

#[derive(Serialize, sqlx::FromRow, ToSchema)]
pub struct ArtistIdentityGroupLibrary {
    pub library_id: String,
    pub recording_count: i64,
    pub release_count: i64,
}

#[derive(Serialize, ToSchema)]
pub struct ArtistIdentityGroupMember {
    pub recording: Recording,
    pub artist_id: String,
    pub association_id: String,
    pub choice_revision: i64,
}

#[derive(Serialize, ToSchema)]
pub struct ArtistIdentityGroupDetail {
    pub group: ArtistIdentityGroup,
    pub libraries: Page<ArtistIdentityGroupLibrary>,
    pub recordings: Page<ArtistIdentityGroupMember>,
    pub releases: Page<Release>,
}

#[derive(Default, Deserialize, ToSchema)]
pub struct ArtistIdentityGroupQuery {
    pub q: Option<String>,
    pub library: Option<String>,
    pub offset: Option<i64>,
    pub limit: Option<i64>,
}

#[derive(Default, Deserialize, ToSchema)]
pub struct ArtistIdentityGroupBrowse {
    pub library: Option<String>,
    pub library_offset: Option<i64>,
    pub recording_offset: Option<i64>,
    pub release_offset: Option<i64>,
    pub limit: Option<i64>,
}

#[derive(Clone, Deserialize, Serialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct ApplyArtistIdentityToCredit {
    pub request_id: Uuid,
    pub group_id: String,
    pub after: Option<String>,
    pub limit: Option<i64>,
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize, ToSchema)]
pub struct ArtistIdentityCreditReceipt {
    pub request_id: Uuid,
    pub artist_id: String,
    pub group_id: String,
    pub applied: i64,
    pub unchanged: i64,
    pub skipped_different: i64,
    pub skipped_unavailable: i64,
    pub next_after: Option<String>,
}
