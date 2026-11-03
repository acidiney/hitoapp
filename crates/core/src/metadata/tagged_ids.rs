use crate::{Actor, Browse, Result, catalog::releases::selected};
use serde::Serialize;
use sqlx::SqliteConnection;
use utoipa::ToSchema;

const MAX_TRACKS: usize = 1000;

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, ToSchema)]
pub struct ReleaseMusicBrainzIds {
    #[schema(required = true)]
    pub release_id: Option<String>,
    #[schema(required = true)]
    pub release_group_id: Option<String>,
}

pub(crate) async fn tagged_release(
    connection: &mut SqliteConnection,
    actor: &Actor,
    release: &str,
) -> Result<ReleaseMusicBrainzIds> {
    let mut query = selected(actor, &Browse::default(), Some(release))?;
    query.push(
        "SELECT DISTINCT m.recording_id, rel.mbid, grp.mbid FROM entries m
         JOIN recordings r ON r.id = m.recording_id
         LEFT JOIN recording_musicbrainz_observations rel ON rel.observation_id = r.metadata_observation_id
            AND rel.entity = 'release' AND rel.release_id = m.release_id
         LEFT JOIN recording_musicbrainz_observations grp ON grp.observation_id = r.metadata_observation_id
            AND grp.entity = 'release_group' AND grp.release_id = m.release_id
         LIMIT 1002",
    );
    let rows: Vec<(String, Option<String>, Option<String>)> =
        query.build_query_as().fetch_all(connection).await?;
    if rows.is_empty() || rows.len() > MAX_TRACKS {
        return Ok(ReleaseMusicBrainzIds::default());
    }
    Ok(ReleaseMusicBrainzIds {
        release_id: unanimous(rows.iter().map(|row| row.1.as_deref())),
        release_group_id: unanimous(rows.iter().map(|row| row.2.as_deref())),
    })
}

fn unanimous<'a>(mut values: impl Iterator<Item = Option<&'a str>>) -> Option<String> {
    let first = values.next()??;
    values
        .all(|value| value == Some(first))
        .then(|| first.to_owned())
}
