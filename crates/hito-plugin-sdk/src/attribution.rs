use crate::content::text;
use serde::{Deserialize, Serialize};
use url::Url;

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(utoipa::ToSchema))]
#[serde(deny_unknown_fields)]
pub struct RecordingAttribution {
    pub creators: Vec<String>,
    pub provider_label: String,
    pub content_url: String,
}

impl RecordingAttribution {
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.creators.is_empty() || self.creators.len() > 16 {
            return Err("Declare between one and sixteen recording creators");
        }
        for creator in &self.creators {
            text(creator, 2048)?;
        }
        text(&self.provider_label, 256)?;
        let value = &self.content_url;
        const INVALID: &str =
            "Recording backlinks must use canonical HTTPS without credentials, query, or fragment";
        if value.len() > 2048
            || value.contains('\\')
            || value
                .chars()
                .any(|character| character.is_control() || character.is_whitespace())
        {
            return Err(INVALID);
        }
        let url = Url::parse(value).map_err(|_| INVALID)?;
        if url.scheme() != "https"
            || url.host().is_none()
            || !url.username().is_empty()
            || url.password().is_some()
            || url.query().is_some()
            || url.fragment().is_some()
            || url.as_str() != value
        {
            return Err(INVALID);
        }
        Ok(())
    }
}
