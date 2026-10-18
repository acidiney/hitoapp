use crate::{
    Actor, HostDeviceId, Result,
    catalog::{SourceAccess, push_source_access},
};
use serde::{Deserialize, Serialize};
use sqlx::{QueryBuilder, SqliteConnection};

#[derive(Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum Provider {
    Youtube,
    Jamendo,
}

impl Provider {
    pub(super) fn as_str(self) -> &'static str {
        match self {
            Self::Youtube => "youtube",
            Self::Jamendo => "jamendo",
        }
    }
}

#[derive(Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct PublicSource {
    pub(super) provider: Provider,
    pub(super) recording_id: String,
}

pub(super) async fn resolve(
    connection: &mut SqliteConnection,
    actor: &Actor,
    host: &HostDeviceId,
    source: &str,
) -> Result<Option<PublicSource>> {
    let mut query = QueryBuilder::new(
        "SELECT json_extract(installation.manifest,'$.id'),item.provider_recording_id
         FROM sources s JOIN recordings r ON r.id=s.recording_id
         JOIN library_access_grants access ON access.library_id=r.library_id
         JOIN provider_sources remote ON remote.source_id=s.id AND remote.recording_id=s.recording_id
         JOIN provider_items item ON item.id=remote.binding_id AND item.installation_id=remote.installation_id
            AND item.recording_id=r.id AND item.retired=0
         JOIN plugin_installations installation ON installation.id=item.installation_id
            AND installation.user_id=access.user_id
         WHERE s.provider='plugin' AND s.id=",
    );
    query
        .push_bind(source.to_owned())
        .push(" AND access.user_id=")
        .push_bind(actor.id().to_owned())
        .push(" AND ");
    push_source_access(&mut query, host, actor.id(), SourceAccess::Playable);
    let row: Option<(String, String)> = query.build_query_as().fetch_optional(connection).await?;
    Ok(row.and_then(|(provider, id)| match provider.as_str() {
        "org.hito.youtube"
            if id.len() == 11
                && id
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || b"_-".contains(&byte)) =>
        {
            Some(PublicSource {
                provider: Provider::Youtube,
                recording_id: id,
            })
        }
        "org.hito.jamendo" => {
            let id = id.strip_prefix("jamendo:track:v1:")?;
            (id.len() <= 20
                && !id.starts_with('0')
                && !id.is_empty()
                && id.bytes().all(|byte| byte.is_ascii_digit())
                && id.parse::<u64>().is_ok())
            .then(|| PublicSource {
                provider: Provider::Jamendo,
                recording_id: id.to_owned(),
            })
        }
        _ => None,
    }))
}
