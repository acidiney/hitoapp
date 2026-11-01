use hito_plugin_sdk::FeaturedKind;
use serde::Serialize;
use utoipa::ToSchema;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, ToSchema)]
pub struct FeaturedEntry {
    pub installation_id: String,
    pub installation_revision: i64,
    pub provider: String,
    pub id: String,
    pub title: String,
    pub subtitle: Option<String>,
    pub kind: FeaturedKind,
    pub reference: String,
    pub artwork_id: Option<String>,
    pub item_count: Option<u32>,
}

#[derive(Debug, Serialize, ToSchema)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum FeaturedShelf {
    Available { items: Vec<FeaturedEntry> },
    Disabled,
    Unsupported,
    Unavailable,
}

impl FeaturedShelf {
    pub(crate) fn outcome(&self) -> &'static str {
        match self {
            Self::Available { .. } => "available",
            Self::Disabled => "disabled",
            Self::Unsupported => "unsupported",
            Self::Unavailable => "unavailable",
        }
    }
}
