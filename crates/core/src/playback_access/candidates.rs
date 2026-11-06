use super::{PlaybackAccess, PlaybackSource};
use crate::{
    Actor, Error, Result,
    catalog::{
        MAX_PLAYBACK_BINDINGS, Origin, SourceAccess, SourceRanking, push_source_access, ranking_on,
    },
    identity::recording_access,
};
use sqlx::QueryBuilder;
use std::{collections::VecDeque, time::Duration};

#[derive(Clone, sqlx::FromRow)]
struct ProviderSnapshot {
    binding_id: String,
    installation_id: String,
    revision: i64,
    generation: i64,
}

#[derive(Clone)]
enum Stage {
    Local,
    Download,
    Podcast,
    Provider(ProviderSnapshot),
}

impl Stage {
    fn origin(&self) -> Origin<'_> {
        match self {
            Self::Local => Origin::LocalFiles,
            Self::Download => Origin::SavedCopies,
            Self::Podcast => Origin::Podcast,
            Self::Provider(snapshot) => Origin::Provider(&snapshot.installation_id),
        }
    }
}

pub const MAX_FALLBACK_ATTEMPTS: u64 = 6;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CandidatesEnd {
    Exhausted,
    FallbackOff,
    AttemptLimit,
}

pub struct PlaybackCandidates {
    playback: PlaybackAccess,
    actor: Actor,
    recording: String,
    remaining: u64,
    stages: VecDeque<Stage>,
    after: Option<String>,
    ceiling: Option<String>,
    podcast: Option<PlaybackSource>,
    fallback: bool,
    offered: u64,
    ended: Option<CandidatesEnd>,
}

impl PlaybackAccess {
    #[tracing::instrument(level = "debug", skip_all, err(Debug), fields(account_id = actor.id()))]
    pub async fn playback_candidates(
        &self,
        actor: &Actor,
        recording: &str,
    ) -> Result<PlaybackCandidates> {
        tokio::time::timeout(Duration::from_secs(1), async {
            actor.check_session(&self.store).await?;
            let mut connection = self.store.pool.acquire().await?;
            recording_access(&mut connection, actor, recording).await?;
            let mut files = QueryBuilder::new(
                "SELECT count(*), max(s.id) FROM sources s INDEXED BY sources_metadata_owner WHERE s.recording_id=",
            );
            files.push_bind(recording.to_owned()).push(" AND s.provider IN ('local','download') AND ");
            push_source_access(&mut files, &self.host, actor.id(), SourceAccess::LocalFile);
            let (count, ceiling): (i64, Option<String>) = files.build_query_as()
                .fetch_one(&mut *connection).await?;
            let providers: Vec<ProviderSnapshot> = sqlx::query_as(
                "SELECT item.id AS binding_id,item.installation_id,item.revision,state.generation FROM provider_items item
                JOIN provider_source_state state ON state.binding_id = item.id
                JOIN plugin_installations p ON p.id = item.installation_id AND p.active = 1
                JOIN plugin_services service ON service.id = p.service_id AND service.active = 1
                JOIN source_choice_bindings permitted ON permitted.binding_id=item.id AND permitted.recording_id=item.recording_id AND permitted.user_id=p.user_id
                WHERE item.recording_id = ? AND item.retired = 0 AND p.user_id = ?
                AND EXISTS(SELECT 1 FROM json_each(p.manifest,'$.capabilities') WHERE value = 'sources')
                ORDER BY item.id LIMIT ?",
            )
            .bind(recording)
            .bind(actor.id())
            .bind(MAX_PLAYBACK_BINDINGS as i64)
            .fetch_all(&mut *connection)
            .await?;
            let mut enclosure = QueryBuilder::new(
                "SELECT s.id FROM current_podcast_sources enclosure CROSS JOIN sources s ON s.id=enclosure.source_id
                WHERE enclosure.recording_id=",
            );
            enclosure.push_bind(recording.to_owned()).push(" AND enclosure.user_id=")
                .push_bind(actor.id().to_owned()).push(" AND ");
            push_source_access(&mut enclosure, &self.host, actor.id(), SourceAccess::Playable);
            enclosure.push(" LIMIT 1");
            let podcast_source: Option<String> = enclosure.build_query_scalar()
                .fetch_optional(&mut *connection).await?;
            let ranking = ranking_on(&mut connection, actor).await?;
            drop(connection);
            let podcast = match podcast_source {
                Some(source) => match self.playback_source(actor, recording, &source).await {
                    Ok(grant) => Some(grant),
                    Err(Error::NotFound) => None,
                    Err(error) => return Err(error),
                },
                None => None,
            };
            Ok(PlaybackCandidates {
                playback: self.clone(),
                actor: actor.clone(),
                recording: recording.to_owned(),
                remaining: count as u64 + (providers.len() * hito_plugin_sdk::MAX_SOURCES_PER_RECORDING) as u64 + u64::from(podcast.is_some()),
                stages: stages(count > 0, podcast.is_some(), providers, &ranking),
                after: None,
                ceiling,
                podcast,
                fallback: ranking.fallback(),
                offered: 0,
                ended: None,
            })
        })
        .await
        .unwrap_or(Err(Error::Upstream(
            "Playback source selection could not finish in time",
        )))
    }
}

impl PlaybackCandidates {
    pub fn ended(&self) -> Option<CandidatesEnd> {
        self.ended
    }

    #[tracing::instrument(level = "debug", skip_all, err(Debug), fields(account_id = self.actor.id()))]
    pub async fn next(&mut self) -> Result<Option<PlaybackSource>> {
        let blocked = self.ended.or(if self.offered > 0 && !self.fallback {
            Some(CandidatesEnd::FallbackOff)
        } else if self.offered >= MAX_FALLBACK_ATTEMPTS {
            Some(CandidatesEnd::AttemptLimit)
        } else {
            None
        });
        if let Some(reason) = blocked {
            self.actor.check_session(&self.playback.store).await?;
            if self.ended.is_none() {
                self.stop(reason);
            }
            return Ok(None);
        }
        let found = tokio::time::timeout(Duration::from_secs(1), self.advance())
            .await
            .unwrap_or(Err(Error::Upstream(
                "Playback source selection could not finish in time",
            )))?;
        let Some(grant) = found else {
            self.stop(CandidatesEnd::Exhausted);
            return Ok(None);
        };
        self.offered += 1;
        if self.offered > 1 {
            tracing::debug!(
                event = "playback_source_fallback",
                attempt = self.offered,
                outcome = "next_source",
                "Previous playback source did not open; offering the next verified source"
            );
        }
        Ok(Some(grant))
    }

    fn stop(&mut self, reason: CandidatesEnd) {
        self.ended = Some(reason);
        tracing::debug!(
            event = "playback_source_fallback",
            attempt = self.offered,
            outcome = match reason {
                CandidatesEnd::Exhausted => "exhausted",
                CandidatesEnd::FallbackOff => "fallback_off",
                CandidatesEnd::AttemptLimit => "attempt_limit",
            },
            "Playback source fallback ended"
        );
    }

    async fn advance(&mut self) -> Result<Option<PlaybackSource>> {
        self.actor.check_session(&self.playback.store).await?;
        let mut connection = self.playback.store.pool.acquire().await?;
        recording_access(&mut connection, &self.actor, &self.recording).await?;
        while self.remaining > 0 {
            let Some(stage) = self.stages.front().cloned() else {
                break;
            };
            if matches!(stage, Stage::Podcast) {
                self.stages.pop_front();
                let Some(podcast) = self.podcast.take() else {
                    continue;
                };
                self.remaining -= 1;
                drop(connection);
                match podcast.check().await {
                    Ok(()) => return Ok(Some(podcast)),
                    Err(Error::NotFound) => {
                        connection = self.playback.store.pool.acquire().await?;
                        continue;
                    }
                    Err(error) => return Err(error),
                }
            }
            let file = !matches!(stage, Stage::Provider(_));
            let mut query = if let Stage::Provider(snapshot) = &stage {
                let mut query = QueryBuilder::new(
                    "SELECT ps.source_id FROM provider_items item
                    CROSS JOIN provider_source_state state ON state.binding_id = item.id
                    CROSS JOIN provider_sources ps INDEXED BY provider_sources_generation
                    ON ps.binding_id=state.binding_id AND ps.generation=state.generation
                    CROSS JOIN sources s ON s.id=ps.source_id WHERE item.retired=0 AND item.recording_id=",
                );
                query
                    .push_bind(self.recording.clone())
                    .push(" AND item.id=")
                    .push_bind(snapshot.binding_id.clone())
                    .push(" AND item.revision=")
                    .push_bind(snapshot.revision)
                    .push(" AND state.generation=")
                    .push_bind(snapshot.generation)
                    .push(" AND s.provider='plugin' AND ");
                push_source_access(
                    &mut query,
                    &self.playback.host,
                    self.actor.id(),
                    SourceAccess::Playable,
                );
                query
            } else {
                let mut query = QueryBuilder::new(
                    "SELECT s.id FROM sources s INDEXED BY sources_metadata_owner WHERE s.recording_id=",
                );
                query
                    .push_bind(self.recording.clone())
                    .push(if matches!(stage, Stage::Local) {
                        " AND s.provider='local' AND "
                    } else {
                        " AND s.provider='download' AND "
                    });
                push_source_access(
                    &mut query,
                    &self.playback.host,
                    self.actor.id(),
                    SourceAccess::LocalFile,
                );
                if let Some(ceiling) = &self.ceiling {
                    query.push(" AND s.id<=").push_bind(ceiling.clone());
                }
                query
            };
            if let Some(after) = &self.after {
                query.push(" AND s.id>").push_bind(after.clone());
            }
            query.push(if file {
                " ORDER BY s.id LIMIT 1"
            } else {
                " ORDER BY ps.source_id LIMIT 1"
            });
            let source: Option<String> = query
                .build_query_scalar()
                .fetch_optional(&mut *connection)
                .await?;
            let Some(source) = source else {
                self.stages.pop_front();
                self.after = None;
                continue;
            };
            self.after = Some(source.clone());
            self.remaining -= 1;
            drop(connection);
            match self
                .playback
                .playback_source(&self.actor, &self.recording, &source)
                .await
            {
                Ok(grant) => return Ok(Some(grant)),
                Err(Error::NotFound) => connection = self.playback.store.pool.acquire().await?,
                Err(error) => return Err(error),
            }
        }
        Ok(None)
    }
}

fn stages(
    files: bool,
    podcast: bool,
    providers: Vec<ProviderSnapshot>,
    ranking: &SourceRanking,
) -> VecDeque<Stage> {
    let mut stages: Vec<Stage> = Vec::new();
    if files {
        stages.extend([Stage::Local, Stage::Download]);
    }
    if podcast {
        stages.push(Stage::Podcast);
    }
    stages.extend(providers.into_iter().map(Stage::Provider));
    stages.sort_by_key(|stage| ranking.position(stage.origin()));
    stages.into()
}
