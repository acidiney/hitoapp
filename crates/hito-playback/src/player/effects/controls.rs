use super::{Target, Update};
use crate::{PlayerError, PlayerPhase, Session, Status};
use std::{future::Future, pin::Pin, sync::Arc, time::Duration};
use uuid::Uuid;

type Pending<T> = Pin<Box<dyn Future<Output = T> + Send>>;
struct Applied {
    seek_id: Option<Uuid>,
    volume: u16,
}
#[derive(Default)]
pub(super) struct Controls {
    configured: bool,
    applied_seek: Option<Uuid>,
    applied_volume: Option<u16>,
    resume_at: Duration,
    duration: Option<Duration>,
    pending: Option<Pending<crate::Result<Applied>>>,
}
impl Controls {
    pub fn busy(&self) -> bool {
        self.pending.is_some()
    }
    pub fn request_seek(&mut self, position_ms: u64) {
        self.resume_at = Duration::from_millis(position_ms);
        self.configured = false;
    }
    pub fn opened(&mut self) {
        self.configured = false;
        self.applied_volume = None;
    }
    pub fn retire(&mut self, snapshot: Option<crate::Snapshot>) {
        if let Some(snapshot) = snapshot {
            if self.configured {
                self.resume_at = snapshot.position;
            }
            self.duration = snapshot.format.and_then(|format| format.duration);
        }
        self.pending = None;
    }
    pub fn sync(&mut self, session: &Arc<Session>, target: &Target) {
        let snapshot = session.snapshot();
        if snapshot.status == Status::Ready
            && !snapshot.ended
            && self.pending.is_none()
            && (!self.configured
                || snapshot.playing != target.playing
                || self.applied_volume != Some(target.volume))
        {
            let session = session.clone();
            let seek_id = target.seek.map(|seek| seek.id);
            let volume = target.volume;
            let seek =
                if self.configured || (self.resume_at.is_zero() && seek_id == self.applied_seek) {
                    None
                } else {
                    Some(self.resume_at)
                };
            let command = if target.playing {
                crate::Command::Resume
            } else {
                crate::Command::Pause
            };
            self.pending = Some(Box::pin(async move {
                session
                    .command(crate::Command::Volume(f32::from(volume) / 1000.))
                    .await?;
                if let Some(position) = seek {
                    session.command(crate::Command::Seek(position)).await?;
                }
                session.command(command).await?;
                Ok(Applied { seek_id, volume })
            }));
        }
    }
    pub async fn next(&mut self) -> crate::Result<()> {
        let result = match self.pending.as_mut() {
            Some(pending) => pending.as_mut().await,
            None => std::future::pending().await,
        };
        self.pending = None;
        let applied = result?;
        self.applied_seek = applied.seek_id;
        self.applied_volume = Some(applied.volume);
        self.configured = true;
        tracing::debug!(event = "player_controls_applied", seek_id = ?applied.seek_id, volume = applied.volume, "Native controls applied");
        Ok(())
    }
    pub fn snapshot(
        &self,
        target: &Target,
        source_id: Option<String>,
        snapshot: crate::Snapshot,
    ) -> Update {
        let phase = match snapshot.status {
            Status::Ready => PlayerPhase::Ready,
            Status::Opening | Status::Starting => PlayerPhase::Opening,
            _ => PlayerPhase::Failed,
        };
        if !self.configured {
            return if phase == PlayerPhase::Failed {
                self.status(target, source_id, phase, Some(PlayerError::Playback))
            } else {
                self.status(target, source_id, PlayerPhase::Opening, None)
            };
        }
        Update {
            key: target.key,
            phase,
            playing: snapshot.playing,
            source_id,
            position_ms: snapshot.position.as_millis().min(u64::MAX as u128) as u64,
            duration_ms: snapshot
                .format
                .and_then(|format| format.duration)
                .map(|duration| duration.as_millis().min(u64::MAX as u128) as u64),
            seek_id: self.applied_seek,
            ended: snapshot.ended,
            exhausted: false,
            error: if phase == PlayerPhase::Failed {
                Some(PlayerError::Playback)
            } else {
                None
            },
        }
    }
    pub fn status(
        &self,
        target: &Target,
        source_id: Option<String>,
        phase: PlayerPhase,
        error: Option<PlayerError>,
    ) -> Update {
        Update {
            key: target.key,
            phase,
            playing: false,
            source_id,
            position_ms: self.resume_at.as_millis().min(u64::MAX as u128) as u64,
            duration_ms: self
                .duration
                .map(|duration| duration.as_millis().min(u64::MAX as u128) as u64),
            seek_id: self.applied_seek,
            ended: false,
            exhausted: false,
            error,
        }
    }
}
