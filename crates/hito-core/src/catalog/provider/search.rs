use super::Catalog;
use crate::{Actor, Error, Result};
use hito_plugin_sdk::{Capability, SearchPage, SearchRequest};
use serde::Deserialize;
use utoipa::ToSchema;

#[derive(Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct ProviderSearch {
    pub installation_revision: i64,
    pub query: String,
    pub cursor: Option<String>,
    pub limit: u16,
}
impl Catalog {
    #[tracing::instrument(level = "info", skip_all, err(Debug), fields(account_id = actor.id()))]
    pub async fn search_provider(
        &self,
        actor: &Actor,
        installation: &str,
        request: ProviderSearch,
    ) -> Result<SearchPage> {
        let search = SearchRequest {
            query: request.query,
            cursor: request.cursor,
            limit: request.limit,
        };
        search
            .validate()
            .map_err(|message| Error::Invalid(message.into()))?;
        let operation = self
            .plugins
            .operation(
                actor,
                installation,
                request.installation_revision,
                &[Capability::Search],
            )
            .await?;
        let page = operation.search(&search).await?;
        let mut connection = self.store.pool.acquire().await?;
        operation.recheck(&mut connection).await?;
        tracing::info!(
            count = page.items.len(),
            has_more = page.next_cursor.is_some(),
            "Provider search completed"
        );
        Ok(page)
    }
}
