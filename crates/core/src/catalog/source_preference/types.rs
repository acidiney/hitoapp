use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum SourcePreferenceEntry {
    LocalFiles,
    SavedCopies,
    Provider { installation_id: String },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, ToSchema)]
pub struct SourcePreferences {
    pub revision: i64,
    pub fallback: bool,
    pub entries: Vec<SourcePreferenceEntry>,
}

#[derive(Clone, Debug, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct UpdateSourcePreferences {
    pub revision: i64,
    pub fallback: bool,
    pub entries: Vec<SourcePreferenceEntry>,
}
