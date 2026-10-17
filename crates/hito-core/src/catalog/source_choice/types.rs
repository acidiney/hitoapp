use crate::Source;
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use uuid::Uuid;

#[derive(Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct ChooseSource {
    pub request_id: Uuid,
    pub source_id: Uuid,
}

#[derive(Serialize, ToSchema)]
pub struct SourceChoiceReceipt {
    pub request_id: Uuid,
    pub recording_id: String,
    pub source_id: Uuid,
    pub revision: i64,
}

#[derive(Serialize, ToSchema)]
pub struct SourceChoice {
    pub recording_id: String,
    pub revision: i64,
    pub mode: SourceChoiceMode,
    pub sources: Vec<SourceChoiceEntry>,
}

#[derive(Serialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum SourceChoiceMode {
    Automatic,
    Manual,
    Reviewed,
}

#[derive(Serialize, sqlx::FromRow, ToSchema)]
pub struct SourceChoiceEntry {
    #[sqlx(flatten)]
    pub source: Source,
    pub selected: bool,
    pub selectable: bool,
}

#[derive(Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct ResetSourceChoice {
    pub request_id: Uuid,
    #[schema(minimum = 0)]
    pub expected_revision: i64,
}

#[derive(Serialize, ToSchema)]
pub struct SourceChoiceResetReceipt {
    pub request_id: Uuid,
    pub recording_id: String,
    pub revision: i64,
}
