use super::{AudiobookPlayback, Collection, check_chapter_limit, continuation, owned_on};
use crate::{Actor, Error, Result, collection::playlist_entries};
use std::time::{Duration, Instant};
use uuid::Uuid;

impl Collection {
    #[tracing::instrument(level = "debug", skip_all, fields(operation = "audiobook_playback_read", request_id = %Uuid::new_v4()))]
    pub async fn audiobook_playback(&self, actor: &Actor, id: &str) -> Result<AudiobookPlayback> {
        let started = Instant::now();
        let result: Result<AudiobookPlayback> =
            tokio::time::timeout(Duration::from_secs(3), async {
                let audiobook_id = Uuid::parse_str(id)
                    .ok()
                    .filter(|id| !id.is_nil())
                    .ok_or(Error::NotFound)?;
                let mut tx = self.store.pool.begin().await?;
                actor.check_session_on(&mut tx).await?;
                owned_on(&mut tx, actor, id).await?;
                check_chapter_limit(&mut tx, id).await?;
                let retained = continuation::read(&mut tx, actor, id)
                    .await?
                    .ok_or(Error::NotFound)?;
                let progress = retained.progress.ok_or(Error::NotFound)?;
                if retained.chapter.audiobook_id != audiobook_id
                    || progress.recording_id != retained.chapter.recording_id
                    || progress.position_ms > 9_007_199_254_740_991
                    || !progress.speed.is_finite()
                    || !(0.25..=4.0).contains(&progress.speed)
                {
                    return Err(Error::Invalid(
                        "The saved audiobook position or speed is unavailable".into(),
                    ));
                }
                let chapters = playlist_entries::read(
                    &mut tx,
                    &self.host,
                    actor,
                    id,
                    playlist_entries::EntryAccess::Authorized,
                )
                .await?;
                let entry_id = retained.chapter.entry_id.to_string();
                let recording_id = retained.chapter.recording_id.to_string();
                if !chapters.iter().any(|chapter| {
                    chapter.entry_id == entry_id
                        && chapter.recording_id.as_deref() == Some(recording_id.as_str())
                        && chapter.available
                }) {
                    return Err(Error::NotFound);
                }
                tx.commit().await?;
                actor.check_session(&self.store).await?;
                Ok(AudiobookPlayback {
                    audiobook_id,
                    chapter: retained.chapter,
                    progress,
                    chapters,
                })
            })
            .await
            .unwrap_or(Err(Error::Upstream(
                "Audiobook playback could not be checked in time",
            )));
        tracing::debug!(
            outcome = result
                .as_ref()
                .map(|_| "prepared")
                .unwrap_or_else(|error| error.code()),
            chapter_count = result
                .as_ref()
                .map(|playback| playback.chapters.len())
                .unwrap_or(0),
            elapsed_ms = started.elapsed().as_millis() as u64,
            "Audiobook playback read completed"
        );
        result
    }
}
