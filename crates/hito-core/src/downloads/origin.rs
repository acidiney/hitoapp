use super::Downloads;
use crate::{Result, plugins::ProviderDownloadOrigin};
use sqlx::{FromRow, QueryBuilder, Row, Sqlite, SqliteConnection, sqlite::SqliteRow};

pub(super) enum DownloadOrigin {
    Provider(ProviderDownloadOrigin),
    Podcast(PodcastDownloadOrigin),
}

pub(crate) enum OriginSelection<T> {
    Available(T),
    Unavailable,
    Restricted,
}

#[derive(sqlx::FromRow)]
pub(super) struct PodcastDownloadOrigin {
    pub podcast_source_id: String,
    pub podcast_episode_id: String,
    pub podcast_subscription_id: String,
    pub podcast_subscription_generation: i64,
    pub podcast_enclosure_revision: i64,
    pub podcast_enclosure_url: String,
}

impl<'row> FromRow<'row, SqliteRow> for DownloadOrigin {
    fn from_row(row: &'row SqliteRow) -> sqlx::Result<Self> {
        match row.try_get::<&str, _>("origin_kind")? {
            "provider" => Ok(Self::Provider(ProviderDownloadOrigin::from_row(row)?)),
            "podcast" => Ok(Self::Podcast(PodcastDownloadOrigin::from_row(row)?)),
            _ => Err(sqlx::Error::Decode(Box::new(std::io::Error::other(
                "Stored download origin is invalid",
            )))),
        }
    }
}

impl Downloads {
    pub(super) async fn capture_origin_on(
        &self,
        connection: &mut SqliteConnection,
        account: &str,
        recording: &str,
    ) -> Result<OriginSelection<DownloadOrigin>> {
        match self
            .plugins
            .capture_download_origin_on(connection, account, recording)
            .await?
        {
            OriginSelection::Available(origin) => {
                return Ok(OriginSelection::Available(DownloadOrigin::Provider(origin)));
            }
            OriginSelection::Restricted => return Ok(OriginSelection::Restricted),
            OriginSelection::Unavailable => {}
        }
        let origin = sqlx::query_as(
            "SELECT enclosure.source_id AS podcast_source_id,enclosure.episode_id AS podcast_episode_id,
                enclosure.subscription_id AS podcast_subscription_id,
                enclosure.subscription_generation AS podcast_subscription_generation,
                enclosure.enclosure_revision AS podcast_enclosure_revision,
                enclosure.enclosure_url AS podcast_enclosure_url
             FROM current_podcast_sources enclosure
             WHERE enclosure.user_id=? AND enclosure.recording_id=?
                AND NOT EXISTS(SELECT 1 FROM recording_source_choices choice
                    WHERE choice.user_id=enclosure.user_id AND choice.recording_id=enclosure.recording_id
                        AND choice.binding_id IS NOT NULL)
             ORDER BY enclosure.source_id LIMIT 1",
        )
        .bind(account)
        .bind(recording)
        .fetch_optional(connection)
        .await?;
        Ok(match origin {
            Some(origin) => OriginSelection::Available(DownloadOrigin::Podcast(origin)),
            None => OriginSelection::Unavailable,
        })
    }
}

impl DownloadOrigin {
    pub(super) fn push_bindings(&self, query: &mut QueryBuilder<'_, Sqlite>) {
        match self {
            Self::Provider(origin) => {
                query
                    .push("'provider',")
                    .push_bind(origin.provider_source_id.clone())
                    .push(",")
                    .push_bind(origin.binding_id.clone())
                    .push(",")
                    .push_bind(origin.binding_revision)
                    .push(",")
                    .push_bind(origin.remote_source_id.clone())
                    .push(",")
                    .push_bind(origin.installation_id.clone())
                    .push(",")
                    .push_bind(origin.installation_revision)
                    .push(",")
                    .push_bind(origin.service_id.clone())
                    .push(",")
                    .push_bind(origin.service_revision)
                    .push(",")
                    .push_bind(origin.credential_digest.clone())
                    .push(",")
                    .push_bind(origin.audio_generation)
                    .push(",NULL,NULL,NULL,NULL,NULL,NULL");
            }
            Self::Podcast(origin) => {
                query
                    .push("'podcast',NULL,NULL,NULL,NULL,NULL,NULL,NULL,NULL,NULL,NULL,")
                    .push_bind(origin.podcast_source_id.clone())
                    .push(",")
                    .push_bind(origin.podcast_episode_id.clone())
                    .push(",")
                    .push_bind(origin.podcast_subscription_id.clone())
                    .push(",")
                    .push_bind(origin.podcast_subscription_generation)
                    .push(",")
                    .push_bind(origin.podcast_enclosure_revision)
                    .push(",")
                    .push_bind(origin.podcast_enclosure_url.clone());
            }
        }
    }
}
