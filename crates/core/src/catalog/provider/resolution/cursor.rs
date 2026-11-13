use super::{
    authority,
    matching::MusicalRecording,
    provider::{Candidate, Page, Subject},
};
use crate::{
    Actor, Catalog, Error, Result,
    catalog::{Origin, ranking_on},
};
use std::{collections::VecDeque, vec::IntoIter};

const MAX_CONNECTIONS: usize = 4;

#[derive(sqlx::FromRow)]
struct Connection {
    id: String,
    revision: i64,
}

#[derive(Default)]
pub(super) struct Candidates {
    pub(super) provider_unavailable: bool,
    pub(super) connection_count: usize,
    query: String,
    connections: IntoIter<Connection>,
    page: Option<Page>,
    finished: VecDeque<Page>,
}

impl Candidates {
    pub(super) async fn new(
        catalog: &Catalog,
        actor: &Actor,
        musical: &MusicalRecording,
    ) -> Result<Self> {
        let Some(query) = musical.query() else {
            return Ok(Self::default());
        };
        let mut connections = sqlx::query_as::<_, Connection>("SELECT installation.id,installation.revision FROM plugin_installations installation
            JOIN plugin_services service ON service.id = installation.service_id AND service.active = 1
            WHERE installation.user_id = ? AND installation.active = 1
                AND EXISTS(SELECT 1 FROM json_each(installation.manifest,'$.capabilities') WHERE value = 'search')
                AND EXISTS(SELECT 1 FROM json_each(installation.manifest,'$.capabilities') WHERE value = 'sources')
            ORDER BY installation.rowid")
            .bind(actor.id()).fetch_all(&catalog.store.pool).await?;
        let ranking = {
            let mut connection = catalog.store.pool.acquire().await?;
            ranking_on(&mut connection, actor).await?
        };
        connections.sort_by_key(|connection| ranking.position(Origin::Provider(&connection.id)));
        connections.truncate(MAX_CONNECTIONS);
        Ok(Self {
            provider_unavailable: false,
            connection_count: connections.len(),
            query,
            connections: connections.into_iter(),
            page: None,
            finished: VecDeque::new(),
        })
    }

    pub(super) async fn next_by_provider_rank(
        &mut self,
        catalog: &Catalog,
        actor: &Actor,
        subject: &Subject<'_>,
    ) -> Result<Option<(usize, Candidate)>> {
        while let Some(page) = self.finished.front_mut() {
            {
                let mut connection = catalog.store.pool.acquire().await?;
                authority::check(&mut connection, actor, subject.recording).await?;
            }
            let ranked = page.next_by_provider_rank(catalog, actor, subject).await;
            self.provider_unavailable |= page.provider_unavailable;
            match ranked {
                Ok(Some(found)) => return Ok(Some(found)),
                Err(Error::Unauthorized) => return Err(Error::Unauthorized),
                Err(error) => {
                    self.provider_unavailable |=
                        matches!(&error, Error::Upstream(_) | Error::ProviderBusy(_));
                    tracing::debug!(
                        event = "source_resolution_provider_failed",
                        error_kind = error.code(),
                        "Source resolution provider exchange failed"
                    );
                }
                Ok(None) => {}
            }
            self.finished.pop_front();
        }
        Ok(None)
    }

    pub(super) async fn next(
        &mut self,
        catalog: &Catalog,
        actor: &Actor,
        subject: &Subject<'_>,
    ) -> Result<Option<Candidate>> {
        loop {
            {
                let mut connection = catalog.store.pool.acquire().await?;
                authority::check(&mut connection, actor, subject.recording).await?;
            }
            if let Some(page) = self.page.as_mut() {
                let candidate = page.next(catalog, actor, subject).await;
                self.provider_unavailable |= page.provider_unavailable;
                match candidate {
                    Ok(Some(candidate)) => return Ok(Some(candidate)),
                    Err(Error::Unauthorized) => return Err(Error::Unauthorized),
                    Err(error) => {
                        self.provider_unavailable |=
                            matches!(&error, Error::Upstream(_) | Error::ProviderBusy(_));
                        tracing::debug!(
                            event = "source_resolution_provider_failed",
                            error_kind = error.code(),
                            "Source resolution provider exchange failed"
                        );
                    }
                    Ok(None) => {}
                }
                self.finished.extend(self.page.take());
                continue;
            }
            let Some(connection) = self.connections.next() else {
                return Ok(None);
            };
            match catalog
                .search_resolution_connection(
                    actor,
                    subject.musical,
                    connection.id,
                    connection.revision,
                    &self.query,
                )
                .await
            {
                Ok(page) => self.page = Some(page),
                Err(Error::Unauthorized) => return Err(Error::Unauthorized),
                Err(error) => {
                    self.provider_unavailable |= super::is_provider_unavailable(&error);
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
