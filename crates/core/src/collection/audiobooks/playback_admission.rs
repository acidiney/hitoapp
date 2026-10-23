use super::{AudiobookChapterAnchor, Collection, admission::check_chapters_on};
use crate::{Actor, Error, Result};
use std::time::{Duration, Instant};

pub struct AudiobookPlaybackAdmission {
    transaction: sqlx::Transaction<'static, sqlx::Sqlite>,
    expires: Instant,
}

impl AudiobookPlaybackAdmission {
    pub fn publish<T>(self, publish: impl FnOnce() -> T) -> Result<T> {
        if Instant::now() >= self.expires {
            return Err(Error::Upstream("Audiobook admission expired"));
        }
        let result = publish();
        drop(self.transaction);
        Ok(result)
    }
}

impl Collection {
    #[tracing::instrument(level = "debug", skip_all, fields(operation = "audiobook_playback_admission", chapter_count = chapters.len()))]
    pub async fn admit_audiobook_playback(
        &self,
        actor: &Actor,
        target: AudiobookChapterAnchor,
        chapters: &[AudiobookChapterAnchor],
    ) -> Result<AudiobookPlaybackAdmission> {
        let started = Instant::now();
        let expires = started + Duration::from_secs(3);
        let result = tokio::time::timeout(Duration::from_secs(3), async {
            if chapters.is_empty()
                || chapters.len() > 10_000
                || !chapters.contains(&target)
                || chapters.iter().any(|chapter| {
                    chapter.audiobook_id != target.audiobook_id
                        || chapter.audiobook_id.is_nil()
                        || chapter.entry_id.is_nil()
                        || chapter.recording_id.is_nil()
                })
            {
                return Err(Error::Invalid(
                    "The audiobook playback selection is invalid".into(),
                ));
            }
            let mut transaction = self.store.pool.begin_with("BEGIN IMMEDIATE").await?;
            actor.check_session_on(&mut transaction).await?;
            for chunk in chapters.chunks(1000) {
                check_chapters_on(&mut transaction, actor, chunk).await?;
            }
            let selected = crate::catalog::recordings_on(
                &mut transaction,
                &self.host,
                actor,
                &[target.recording_id],
            )
            .await?;
            if !selected[0].available {
                return Err(Error::NotFound);
            }
            actor.check_session_on(&mut transaction).await?;
            tracing::debug!(
                event = "audiobook_admission_ready",
                elapsed_ms = started.elapsed().as_millis() as u64,
                "Audiobook authority retained for publication"
            );
            Ok(AudiobookPlaybackAdmission {
                transaction,
                expires,
            })
        })
        .await
        .unwrap_or(Err(Error::Upstream(
            "Audiobook admission could not be checked in time",
        )));
        tracing::debug!(
            success = result.is_ok(),
            elapsed_ms = started.elapsed().as_millis() as u64,
            "Audiobook playback admission completed"
        );
        result
    }
}
