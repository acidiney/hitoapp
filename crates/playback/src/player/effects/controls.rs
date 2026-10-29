use super::super::loading::{Restoration, Restored};
use super::{Target, Update};
use crate::{PlayerError, PlayerPhase, PlayerSpeedSupport, Session, Status};
use hito_audio::SpeedCapability;
use std::{future::Future, pin::Pin, sync::Arc, time::Duration};
use uuid::Uuid;

type Pending<T> = Pin<Box<dyn Future<Output = T> + Send>>;
struct Applied {
    restored: Option<Restored>,
    seek_id: Option<Uuid>,
    volume: u16,
    speed: u16,
}
#[derive(Default)]
pub(super) struct Controls {
    configured: bool,
    restoration: Option<Restoration>,
    restored: Option<Restored>,
    applied_seek: Option<Uuid>,
    applied_volume: Option<u16>,
    applied_speed: Option<u16>,
    support: Option<PlayerSpeedSupport>,
    resume_at: Duration,
    duration: Option<Duration>,
    pending: Option<Pending<crate::Result<Applied>>>,
}
impl Controls {
    pub fn busy(&self) -> bool {
        self.pending.is_some()
    }
    pub fn request_restoration(&mut self, restoration: Restoration) {
        self.request_seek(restoration.position_ms);
        self.restoration = Some(restoration);
    }
    pub fn request_seek(&mut self, position_ms: u64) {
        self.restoration = None;
        self.restored = None;
        self.resume_at = Duration::from_millis(position_ms);
        self.configured = false;
    }
    pub fn opened(&mut self) {
        self.configured = false;
        self.applied_volume = None;
        self.applied_speed = Some(1000);
        self.support = None;
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
        let capability = snapshot.format.map(|format| format.speed_capability());
        let negotiated = negotiate(capability, target.speed);
        if snapshot.status == Status::Ready
            && let Some(capability) = capability
        {
            self.support = Some(support(capability));
        }
        if snapshot.status == Status::Ready
            && (!snapshot.ended || !self.configured)
            && self.pending.is_none()
            && (!self.configured
                || snapshot.playing != target.playing
                || self.applied_volume != Some(target.volume)
                || self.applied_speed != Some(negotiated))
        {
            let session = session.clone();
            let seek_id = target.seek.map(|seek| seek.id);
            let volume = target.volume;
            let restoration = self.restoration;
            let restored = self.restored;
            let speed = negotiated;
            let change_speed = self.applied_speed != Some(speed);
            if speed != target.speed {
                tracing::debug!(
                    event = "player_speed_unsupported_by_source",
                    chosen = target.speed,
                    effective = speed,
                    "The current source plays at unit speed"
                );
            }
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
                let restored = if let Some(restoration) = restoration {
                    Some(Restored {
                        id: restoration.id,
                        baseline: session.snapshot().position,
                    })
                } else {
                    restored
                };
                if change_speed {
                    session
                        .command(crate::Command::Speed(f32::from(speed) / 1000.))
                        .await?;
                }
                session.command(command).await?;
                Ok(Applied {
                    restored,
                    seek_id,
                    volume,
                    speed,
                })
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
        self.restored = applied.restored;
        self.restoration = None;
        self.applied_seek = applied.seek_id;
        self.applied_volume = Some(applied.volume);
        self.applied_speed = Some(applied.speed);
        self.configured = true;
        tracing::debug!(event = "player_controls_applied", seek_id = ?applied.seek_id, volume = applied.volume, speed = applied.speed, "Native controls applied");
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
            speed: snapshot.speed,
            speed_support: snapshot
                .format
                .map(|format| support(format.speed_capability())),
            source_id,
            position: snapshot.position,
            duration_ms: snapshot
                .format
                .and_then(|format| format.duration)
                .map(|duration| duration.as_millis().min(u64::MAX as u128) as u64),
            seek_id: self.applied_seek,
            restored: self.restored,
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
            speed: self.applied_speed.unwrap_or(1000),
            speed_support: self.support,
            source_id,
            position: self.resume_at,
            duration_ms: self
                .duration
                .map(|duration| duration.as_millis().min(u64::MAX as u128) as u64),
            seek_id: self.applied_seek,
            restored: self.restored,
            ended: false,
            exhausted: false,
            error,
        }
    }
}

/// The speed to present for a source: the chosen speed when the source supports
/// it, otherwise unit speed. The chosen speed is never forgotten, so the next
/// capable source restores it.
fn negotiate(capability: Option<SpeedCapability>, chosen: u16) -> u16 {
    match capability {
        Some(capability) if !capability.supports(f32::from(chosen) / 1000.) => 1000,
        _ => chosen,
    }
}

fn support(capability: SpeedCapability) -> PlayerSpeedSupport {
    match capability {
        SpeedCapability::UnitOnly => PlayerSpeedSupport {
            min: 1000,
            max: 1000,
        },
        SpeedCapability::Range { min, max } => PlayerSpeedSupport {
            min: (min * 1000.).round() as u16,
            max: (max * 1000.).round() as u16,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const RANGE: SpeedCapability = SpeedCapability::Range { min: 0.5, max: 3. };

    #[test]
    fn a_supported_speed_is_kept_and_an_unsupported_one_plays_at_unit_speed() {
        assert_eq!(negotiate(Some(RANGE), 500), 500);
        assert_eq!(negotiate(Some(RANGE), 3000), 3000);
        assert_eq!(negotiate(Some(SpeedCapability::UnitOnly), 1500), 1000);
        assert_eq!(negotiate(Some(SpeedCapability::UnitOnly), 1000), 1000);
        assert_eq!(negotiate(None, 1500), 1500);
    }

    #[test]
    fn the_chosen_speed_returns_when_a_capable_source_follows_an_incapable_one() {
        let chosen = 1500;
        let sources = [Some(RANGE), Some(SpeedCapability::UnitOnly), Some(RANGE)];
        let presented: Vec<u16> = sources
            .into_iter()
            .map(|capability| negotiate(capability, chosen))
            .collect();
        assert_eq!(presented, [1500, 1000, 1500]);
    }

    #[test]
    fn support_is_reported_in_thousandths() {
        assert_eq!(
            support(RANGE),
            PlayerSpeedSupport {
                min: 500,
                max: 3000
            }
        );
        assert_eq!(
            support(SpeedCapability::UnitOnly),
            PlayerSpeedSupport {
                min: 1000,
                max: 1000
            }
        );
    }
}
