use super::{
    MetadataProvider, ReadSimilarArtists, SimilarArtistsAvailability,
    identity::groups::{Groups, group_summary, visible_in},
};
use crate::{
    Actor, ArtistCatalog, Error, Recording, Result,
    catalog::{push_available, recordings_on},
};
use rand::seq::SliceRandom;
use serde::Serialize;
use std::time::Instant;
use utoipa::ToSchema;
use uuid::Uuid;

const RADIO_TRACKS: usize = 50;
const SIMILAR_ARTISTS: u16 = 25;
const TRACKS_PER_SIMILAR_ARTIST: i64 = 5;

struct Pick {
    recording: String,
    group: String,
}

#[derive(Serialize, ToSchema)]
pub struct ArtistRadio {
    pub artist: String,
    pub similar_artists: i64,
    pub tracks: Vec<Recording>,
}

impl ArtistCatalog {
    #[tracing::instrument(level = "debug", skip_all, fields(operation = "artist_radio", request_id = %Uuid::new_v4()))]
    pub async fn artist_radio(&self, actor: &Actor, group: &str) -> Result<ArtistRadio> {
        let started = Instant::now();
        let result = self.read_artist_radio(actor, group).await;
        tracing::debug!(
            outcome = result
                .as_ref()
                .map_or_else(|error| error.code(), |_| "success"),
            tracks = result.as_ref().map_or(0, |radio| radio.tracks.len()),
            similar_artists = result.as_ref().map_or(0, |radio| radio.similar_artists),
            elapsed_ms = started.elapsed().as_millis() as u64,
            "Artist radio built"
        );
        result
    }

    async fn read_artist_radio(&self, actor: &Actor, group: &str) -> Result<ArtistRadio> {
        let summary = {
            let mut tx = self.store.pool.begin().await?;
            actor.check_session_on(&mut tx).await?;
            group_summary(&mut tx, actor, group)
                .await?
                .ok_or(Error::NotFound)?
        };
        let similar = if summary.provider == MetadataProvider::BuiltinMusicBrainz {
            self.similar_groups(actor, &summary).await?
        } else {
            Vec::new()
        };
        let mut tx = self.store.pool.begin().await?;
        actor.check_session_on(&mut tx).await?;
        let own: Vec<String> = self
            .sample(actor, Groups::One(group), None, &mut tx)
            .await?
            .into_iter()
            .map(|pick| pick.recording)
            .collect();
        let others: Vec<Pick> = if similar.is_empty() {
            Vec::new()
        } else {
            self.sample(
                actor,
                Groups::Many(&similar),
                Some(TRACKS_PER_SIMILAR_ARTIST),
                &mut tx,
            )
            .await?
        };
        let mixed = blend(own, others, RADIO_TRACKS);
        let ids = mixed
            .ids
            .iter()
            .map(|id| {
                Uuid::parse_str(id)
                    .map_err(|_| Error::Internal("Stored recording identity is invalid".into()))
            })
            .collect::<Result<Vec<_>>>()?;
        let tracks = recordings_on(&mut tx, &self.host, actor, &ids).await?;
        actor.check_session_on(&mut tx).await?;
        tx.commit().await?;
        Ok(ArtistRadio {
            artist: summary.name.value,
            similar_artists: mixed.similar_groups as i64,
            tracks,
        })
    }

    async fn similar_groups(
        &self,
        actor: &Actor,
        summary: &super::ArtistIdentityGroup,
    ) -> Result<Vec<String>> {
        let answer = self
            .recording_similar_artists_until(
                actor,
                &summary.name.recording_id,
                &summary.name.association_id,
                ReadSimilarArtists {
                    limit: SIMILAR_ARTISTS,
                },
                std::future::pending(),
            )
            .await;
        match answer {
            Ok(SimilarArtistsAvailability::Available { artists, .. }) => Ok(artists
                .into_iter()
                .filter_map(|artist| artist.chosen_artist_id)
                .filter(|id| *id != summary.id)
                .collect()),
            Ok(_) => Ok(Vec::new()),
            Err(
                Error::Conflict(_)
                | Error::ProviderBusy(_)
                | Error::OperationRejected(_)
                | Error::Upstream(_)
                | Error::NotFound,
            ) => Ok(Vec::new()),
            Err(error) => Err(error),
        }
    }

    async fn sample(
        &self,
        actor: &Actor,
        groups: Groups<'_>,
        per_group: Option<i64>,
        tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    ) -> Result<Vec<Pick>> {
        let mut query = visible_in(actor, None, groups);
        query.push(
            "SELECT recording_id, group_id FROM (SELECT v.recording_id, v.group_id,
                row_number() OVER (PARTITION BY v.group_id ORDER BY random()) AS n
                FROM visible v JOIN recordings r ON r.id = v.recording_id WHERE ",
        );
        push_available(&mut query, &self.host, actor.id());
        query
            .push(") WHERE n <= ")
            .push_bind(per_group.unwrap_or(RADIO_TRACKS as i64))
            .push(" ORDER BY random() LIMIT ")
            .push_bind(RADIO_TRACKS as i64);
        let rows: Vec<(String, String)> = query.build_query_as().fetch_all(&mut **tx).await?;
        Ok(rows
            .into_iter()
            .map(|(recording, group)| Pick { recording, group })
            .collect())
    }
}

struct Mixed {
    ids: Vec<String>,
    similar_groups: usize,
}

fn blend(own: Vec<String>, others: Vec<Pick>, limit: usize) -> Mixed {
    let own_share = if others.is_empty() {
        limit
    } else {
        limit * 2 / 5
    };
    let mut chosen: Vec<String> = own.iter().take(own_share).cloned().collect();
    chosen.extend(
        others
            .iter()
            .take(limit - chosen.len())
            .map(|pick| pick.recording.clone()),
    );
    let room = limit - chosen.len();
    chosen.extend(own.into_iter().skip(own_share).take(room));
    let mut seen = std::collections::HashSet::new();
    chosen.retain(|id| seen.insert(id.clone()));
    let similar_groups = others
        .iter()
        .filter(|pick| seen.contains(&pick.recording))
        .map(|pick| pick.group.as_str())
        .collect::<std::collections::HashSet<_>>()
        .len();
    chosen.shuffle(&mut rand::thread_rng());
    Mixed {
        ids: chosen,
        similar_groups,
    }
}

#[cfg(test)]
mod tests {
    use super::{Pick, blend};

    fn ids(prefix: &str, count: usize) -> Vec<String> {
        (0..count).map(|n| format!("{prefix}{n}")).collect()
    }

    fn picks(prefix: &str, group: &str, count: usize) -> Vec<Pick> {
        ids(prefix, count)
            .into_iter()
            .map(|recording| Pick {
                recording,
                group: group.into(),
            })
            .collect()
    }

    #[test]
    fn the_artist_alone_fills_the_queue_up_to_the_bound() {
        let mixed = blend(ids("own", 80), Vec::new(), 50);
        assert_eq!(mixed.ids.len(), 50);
        assert_eq!(mixed.similar_groups, 0);
    }

    #[test]
    fn similar_artists_take_three_fifths_and_short_sides_are_topped_up() {
        let mut others = picks("a", "g1", 40);
        others.extend(picks("b", "g2", 40));
        let mixed = blend(ids("own", 80), others, 50);
        assert_eq!(
            mixed.ids.iter().filter(|id| id.starts_with("own")).count(),
            20
        );
        assert_eq!(mixed.ids.len(), 50);
        let mixed = blend(ids("own", 80), picks("other", "g1", 4), 50);
        assert_eq!(
            mixed
                .ids
                .iter()
                .filter(|id| id.starts_with("other"))
                .count(),
            4
        );
        assert_eq!(mixed.ids.len(), 50);
        let mixed = blend(ids("own", 3), picks("other", "g1", 80), 50);
        assert_eq!(mixed.ids.len(), 50);
        assert_eq!(
            mixed.ids.iter().filter(|id| id.starts_with("own")).count(),
            3
        );
    }

    #[test]
    fn only_similar_artists_that_reach_the_queue_are_counted() {
        let mut others = picks("a", "g1", 30);
        others.extend(picks("b", "g2", 5));
        let mixed = blend(ids("own", 80), others, 50);
        assert_eq!(
            mixed.similar_groups, 1,
            "the second artist's tracks were cut off"
        );
        let mut others = picks("a", "g1", 10);
        others.extend(picks("b", "g2", 5));
        assert_eq!(blend(ids("own", 80), others, 50).similar_groups, 2);
    }

    #[test]
    fn nothing_repeats() {
        let mixed = blend(ids("x", 10), picks("x", "g1", 10), 50);
        assert_eq!(mixed.ids.len(), 10);
    }
}
