use super::CatalogSort;
use super::search::{scoped_terms, search_terms};
use crate::{Actor, Result};
use serde::Deserialize;
use sqlx::{QueryBuilder, Sqlite};
use utoipa::ToSchema;

#[derive(Default, Deserialize, ToSchema)]
pub struct Browse {
    pub sort: Option<CatalogSort>,
    pub q: Option<String>,
    pub library: Option<String>,
    pub artist: Option<String>,
    pub offset: Option<i64>,
    pub recording_offset: Option<i64>,
    pub featured_offset: Option<i64>,
    pub limit: Option<i64>,
}

impl Browse {
    pub(super) fn bounds(&self) -> (i64, i64) {
        (
            self.offset.unwrap_or(0).max(0),
            self.limit.unwrap_or(60).clamp(1, 200),
        )
    }

    pub(super) fn filter(
        &self,
        query: &mut QueryBuilder<'_, Sqlite>,
        actor: &Actor,
        alias: &str,
        index: &str,
        columns: &str,
    ) -> Result<()> {
        query
            .push(" WHERE EXISTS (SELECT 1 FROM library_members access WHERE access.library_id = ")
            .push(alias)
            .push(".library_id AND access.user_id = ")
            .push_bind(actor.id().to_owned())
            .push(")");
        query
            .push(" AND ")
            .push(alias)
            .push(".user_id = ")
            .push_bind(actor.id().to_owned());
        if let Some(library) = &self.library {
            query
                .push(" AND ")
                .push(alias)
                .push(".library_id = ")
                .push_bind(library.clone());
        }
        let terms = search_terms(self.q.as_deref().unwrap_or_default())?;
        if !terms.is_empty() {
            query
                .push(" AND ")
                .push(alias)
                .push(".rowid IN (SELECT rowid FROM ")
                .push(index)
                .push(" WHERE ")
                .push(index)
                .push(" MATCH ")
                .push_bind(scoped_terms(actor, columns, &terms))
                .push(")");
        }
        Ok(())
    }
}
