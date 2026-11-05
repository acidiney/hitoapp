#[cfg(feature = "device")]
mod controls;
#[cfg(feature = "device")]
mod lookahead;
#[cfg(feature = "device")]
pub(super) mod native;
#[cfg(feature = "device")]
mod sources;
#[cfg(feature = "device")]
pub(super) struct Target {
    pub key: Key,
    pub actor: hito_core::Actor,
    pub recording: Uuid,
    pub playing: bool,
    pub play_revision: u64,
    pub seek: Option<super::types::Seek>,
    pub restoration: Option<super::loading::Restoration>,
    pub output: bool,
    pub volume: u16,
    pub speed: u16,
    pub permitted: bool,
    pub next: Option<NextTarget>,
}
#[cfg(feature = "device")]
#[derive(Clone, Copy)]
pub(super) struct NextTarget {
    pub key: Key,
    pub recording: Uuid,
}

use super::{PlayerError, PlayerPhase, ownership::State};
use std::time::Duration;
use uuid::Uuid;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) struct Key {
    pub scope: Uuid,
    pub selection: u64,
    pub entry: Uuid,
    pub start: Option<Uuid>,
}
pub(super) struct Update {
    pub key: Key,
    pub phase: PlayerPhase,
    pub playing: bool,
    pub speed: u16,
    pub speed_support: Option<super::PlayerSpeedSupport>,
    pub source_id: Option<String>,
    pub position: Duration,
    pub duration_ms: Option<u64>,
    pub seek_id: Option<Uuid>,
    pub restored: Option<super::loading::Restored>,
    pub ended: bool,
    pub exhausted: bool,
    pub error: Option<PlayerError>,
}
impl Update {
    pub fn position_ms(&self) -> u64 {
        self.position.as_millis().min(u64::MAX as u128) as u64
    }
}
pub(super) enum Effects {
    Disabled,
    #[cfg(feature = "device")]
    Native(Box<native::Native>),
}
impl Effects {
    #[cfg(feature = "device")]
    pub fn native(catalog: hito_core::Catalog, playback: hito_core::PlaybackAccess) -> Self {
        Self::Native(Box::new(native::Native::new(catalog, playback)))
    }
    pub fn enabled(&self) -> bool {
        !matches!(self, Self::Disabled)
    }
    pub fn sync(&mut self, state: &State, can_play: bool) {
        match self {
            Self::Disabled => {
                let _ = (state, can_play);
            }
            #[cfg(feature = "device")]
            Self::Native(native) => native.sync(state.target().map(|mut target| {
                target.output &= can_play;
                target
            })),
        }
    }
    pub async fn next(&mut self) -> Update {
        match self {
            Self::Disabled => std::future::pending().await,
            #[cfg(feature = "device")]
            Self::Native(native) => native.next().await,
        }
    }
    pub async fn close(&mut self) {
        match self {
            Self::Disabled => (),
            #[cfg(feature = "device")]
            Self::Native(native) => native.close().await,
        }
    }
}
