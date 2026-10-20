use super::{
    authority::{self, Target},
    provider::{Candidate, Page},
};
use crate::{Actor, Catalog, Error, Result};
use std::vec::IntoIter;

#[derive(sqlx::FromRow)]
struct Connection {
    id: String,
    revision: i64,
}

#[derive(Default)]
pub(super) struct Candidates {
    pub(super) provider_unavailable: bool,
    query: String,
    connections: IntoIter<Connection>,
    page: Option<Page>,
}

impl Candidates {
    pub(super) async fn new(catalog: &Catalog, actor: &Actor, target: &Target) -> Result<Self> {
        let Some(query) = target.musical.query() else {
            return Ok(Self::default());
        };
        let connections = sqlx::query_as::<_, Connection>("SELECT installation.id,installation.revision FROM plugin_installations installation
            JOIN plugin_services service ON service.id = installation.service_id AND service.active = 1
            WHERE installation.user_id = ? AND installation.active = 1
                AND EXISTS(SELECT 1 FROM json_each(installation.manifest,'$.capabilities') WHERE value = 'search')
                AND EXISTS(SELECT 1 FROM json_each(installation.manifest,'$.capabilities') WHERE value = 'sources')
            ORDER BY installation.id LIMIT 4")
            .bind(actor.id()).fetch_all(&catalog.store.pool).await?;
        Ok(Self {
            provider_unavailable: false,
            query,
            connections: connections.into_iter(),
            page: None,
        })
    }

    pub(super) async fn next(
        &mut self,
        catalog: &Catalog,
        actor: &Actor,
        recording: &str,
        target: &Target,
    ) -> Result<Option<Candidate>> {
        loop {
            {
                let mut connection = catalog.store.pool.acquire().await?;
                authority::access(&mut connection, actor, recording).await?;
            }
            if let Some(page) = self.page.as_mut() {
                let candidate = page.next(catalog, actor, recording, target).await;
                self.provider_unavailable |= page.provider_unavailable;
                match candidate {
                    Ok(Some(candidate)) => return Ok(Some(candidate)),
                    Err(Error::Unauthorized) => return Err(Error::Unauthorized),
                    Err(error) => {
                        self.provider_unavailable |= matches!(&error, Error::Upstream(_));
                        tracing::debug!(
                            event = "source_resolution_provider_failed",
                            error_kind = error.code(),
                            "Source resolution provider exchange failed"
                        );
                    }
                    Ok(None) => {}
                }
                self.page = None;
                continue;
            }
            let Some(connection) = self.connections.next() else {
                return Ok(None);
            };
            match catalog
                .search_resolution_connection(
                    actor,
                    target,
                    connection.id,
                    connection.revision,
                    &self.query,
                )
                .await
            {
                Ok(page) => self.page = Some(page),
                Err(Error::Unauthorized) => return Err(Error::Unauthorized),
                Err(error) => {
                    self.provider_unavailable |= matches!(&error, Error::Upstream(_));
                    tracing::debug!(
                        event = "source_resolution_provider_failed",
                        error_kind = error.code(),
                        "Source resolution provider exchange failed"
                    );
                }
            }
        }
    }
}
