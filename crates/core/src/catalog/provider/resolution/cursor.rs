use super::{
    authority::{self, Traversal},
    matching::MusicalRecording,
    provider::{Candidate, Page, Subject},
};
use crate::{
    Actor, Catalog, Error, PlaybackFailure, Result,
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
    pub(super) failure: Option<PlaybackFailure>,
    pub(super) connection_count: usize,
    query: String,
    connections: IntoIter<Connection>,
    page: Option<Page>,
    finished: VecDeque<Page>,
    stopped: bool,
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
            failure: None,
            connection_count: connections.len(),
            query,
            connections: connections.into_iter(),
            page: None,
            finished: VecDeque::new(),
            stopped: false,
        })
    }

    pub(super) async fn next_by_provider_rank(
        &mut self,
        catalog: &Catalog,
        actor: &Actor,
        subject: &Subject<'_>,
    ) -> Result<Option<(usize, Candidate)>> {
        loop {
            if !self.current(catalog, actor, subject).await? {
                return Ok(None);
            }
            let Some(page) = self.finished.front_mut() else {
                return Ok(None);
            };
            let ranked = page.next_by_provider_rank(catalog, actor, subject).await;
            self.provider_unavailable |= page.provider_unavailable;
            PlaybackFailure::merge(&mut self.failure, page.failure);
            match ranked {
                Ok(Traversal::Current(Some(found))) => return Ok(Some(found)),
                Ok(Traversal::Superseded) => {
                    self.stop();
                    return Ok(None);
                }
                Err(Error::Unauthorized) => return Err(Error::Unauthorized),
                Err(error) => {
                    self.provider_unavailable |= super::is_provider_unavailable(&error);
                    PlaybackFailure::merge(
                        &mut self.failure,
                        PlaybackFailure::from_provider_error(&error),
                    );
                    tracing::debug!(
                        event = "source_resolution_provider_failed",
                        error_kind = error.code(),
                        "Source resolution provider exchange failed"
                    );
                }
                Ok(Traversal::Current(None)) => {}
            }
            self.finished.pop_front();
        }
    }

    pub(super) async fn next(
        &mut self,
        catalog: &Catalog,
        actor: &Actor,
        subject: &Subject<'_>,
    ) -> Result<Option<Candidate>> {
        loop {
            if !self.current(catalog, actor, subject).await? {
                return Ok(None);
            }
            if let Some(page) = self.page.as_mut() {
                let candidate = page.next(catalog, actor, subject).await;
                self.provider_unavailable |= page.provider_unavailable;
                PlaybackFailure::merge(&mut self.failure, page.failure);
                match candidate {
                    Ok(Traversal::Current(Some(candidate))) => return Ok(Some(candidate)),
                    Ok(Traversal::Superseded) => {
                        self.stop();
                        return Ok(None);
                    }
                    Err(Error::Unauthorized) => return Err(Error::Unauthorized),
                    Err(error) => {
                        self.provider_unavailable |= super::is_provider_unavailable(&error);
                        PlaybackFailure::merge(
                            &mut self.failure,
                            PlaybackFailure::from_provider_error(&error),
                        );
                        tracing::debug!(
                            event = "source_resolution_provider_failed",
                            error_kind = error.code(),
                            "Source resolution provider exchange failed"
                        );
                    }
                    Ok(Traversal::Current(None)) => {}
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
                    subject,
                    connection.id,
                    connection.revision,
                    &self.query,
                )
                .await
            {
                Ok(Traversal::Current(page)) => self.page = Some(page),
                Ok(Traversal::Superseded) => {
                    self.stop();
                    return Ok(None);
                }
                Err(Error::Unauthorized) => return Err(Error::Unauthorized),
                Err(error) => {
                    self.provider_unavailable |= super::is_provider_unavailable(&error);
                    PlaybackFailure::merge(
                        &mut self.failure,
                        PlaybackFailure::from_provider_error(&error),
                    );
                    tracing::debug!(
                        event = "source_resolution_provider_failed",
                        error_kind = error.code(),
                        "Source resolution provider exchange failed"
                    );
                }
            }
        }
    }

    async fn current(
        &mut self,
        catalog: &Catalog,
        actor: &Actor,
        subject: &Subject<'_>,
    ) -> Result<bool> {
        if self.stopped {
            return Ok(false);
        }
        let mut tx = catalog.store.pool.begin().await?;
        let current = authority::current(&mut tx, actor, subject.source).await?;
        tx.commit().await?;
        if matches!(current, Traversal::Superseded) {
            self.stop();
            return Ok(false);
        }
        Ok(true)
    }

    fn stop(&mut self) {
        self.stopped = true;
        self.page = None;
        self.finished.clear();
        self.connections = Vec::new().into_iter();
    }
}
