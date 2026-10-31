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

#[derive(Serialize, ToSchema)]
pub struct ArtistRadio {
    pub artist: String,
    /// How many of the account's chosen artists, named as similar, contributed tracks.
    pub similar_artists: i64,
    pub tracks: Vec<Recording>,
}

impl ArtistCatalog {
    /// A shuffled mix of one chosen artist's library tracks and, when a similarity provider
    /// answers, library tracks of the chosen artists it names. Reads only the library, plus one
    /// bounded similarity request that carries an artist identity and a limit.
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
            .await?;
        let others: Vec<String> = if similar.is_empty() {
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
        let similar_artists = if others.is_empty() {
            0
        } else {
            similar.len() as i64
        };
        let ids = blend(own, others, RADIO_TRACKS)
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
            similar_artists,
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

    /// Random playable recordings of the given groups, at most `RADIO_TRACKS`, and at most
    /// `per_group` for each group when that is set.
    async fn sample(
        &self,
        actor: &Actor,
        groups: Groups<'_>,
        per_group: Option<i64>,
        tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    ) -> Result<Vec<String>> {
        let mut query = visible_in(actor, None, groups);
        query.push(
            "SELECT recording_id FROM (SELECT v.recording_id,
                row_number() OVER (PARTITION BY v.group_id ORDER BY random()) AS n
                FROM visible v JOIN recordings r ON r.id = v.recording_id WHERE ",
        );
        push_available(&mut query, &self.host, actor.id());
        query
            .push(") WHERE n <= ")
            .push_bind(per_group.unwrap_or(RADIO_TRACKS as i64))
            .push(" ORDER BY random() LIMIT ")
            .push_bind(RADIO_TRACKS as i64);
        Ok(query.build_query_scalar().fetch_all(&mut **tx).await?)
    }
}

/// Chooses up to `limit` ids: two fifths from the artist's own tracks when other tracks exist
/// (the rest from those), topping up from whichever side has more, then shuffles the result.
fn blend(own: Vec<String>, others: Vec<String>, limit: usize) -> Vec<String> {
    let own_share = if others.is_empty() {
        limit
    } else {
        limit * 2 / 5
    };
    let mut chosen: Vec<String> = own.iter().take(own_share).cloned().collect();
    chosen.extend(others.iter().take(limit - chosen.len()).cloned());
    let room = limit - chosen.len();
    chosen.extend(own.into_iter().skip(own_share).take(room));
    let mut seen = std::collections::HashSet::new();
    chosen.retain(|id| seen.insert(id.clone()));
    chosen.shuffle(&mut rand::thread_rng());
    chosen
}

#[cfg(test)]
mod tests {
    use super::blend;

    fn ids(prefix: &str, count: usize) -> Vec<String> {
        (0..count).map(|n| format!("{prefix}{n}")).collect()
    }

    #[test]
    fn the_artist_alone_fills_the_queue_up_to_the_bound() {
        let mixed = blend(ids("own", 80), Vec::new(), 50);
        assert_eq!(mixed.len(), 50);
    }

    #[test]
    fn similar_artists_take_three_fifths_and_short_sides_are_topped_up() {
        let mixed = blend(ids("own", 80), ids("other", 80), 50);
        assert_eq!(mixed.iter().filter(|id| id.starts_with("own")).count(), 20);
        assert_eq!(mixed.len(), 50);
        let mixed = blend(ids("own", 80), ids("other", 4), 50);
        assert_eq!(mixed.iter().filter(|id| id.starts_with("other")).count(), 4);
        assert_eq!(mixed.len(), 50);
        let mixed = blend(ids("own", 3), ids("other", 80), 50);
        assert_eq!(mixed.len(), 50);
        assert_eq!(mixed.iter().filter(|id| id.starts_with("own")).count(), 3);
    }

    #[test]
    fn nothing_repeats() {
        let mixed = blend(ids("x", 10), ids("x", 10), 50);
        assert_eq!(mixed.len(), 10);
    }
}
