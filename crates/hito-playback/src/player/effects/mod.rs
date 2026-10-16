#[cfg(feature = "device")]
pub(super) mod native;
#[cfg(feature = "device")]
mod sources;
use super::{PlayerError, PlayerPhase, ownership::State};
use uuid::Uuid;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) struct Key {
    pub scope: Uuid,
    pub selection: u64,
    pub entry: Uuid,
}
pub(super) struct Update {
    pub key: Key,
    pub phase: PlayerPhase,
    pub playing: bool,
    pub source_id: Option<String>,
    pub position_ms: u64,
    pub duration_ms: Option<u64>,
    pub ended: bool,
    pub exhausted: bool,
    pub error: Option<PlayerError>,
}
pub(super) enum Effects {
    Disabled,
    #[cfg(feature = "device")]
    Native(Box<native::Native>),
}
impl Effects {
    #[cfg(feature = "device")]
    pub fn native(catalog: hito_core::Catalog) -> Self {
        Self::Native(Box::new(native::Native::new(catalog)))
    }
    pub fn enabled(&self) -> bool {
        !matches!(self, Self::Disabled)
    }
    pub fn sync(&mut self, state: &State) {
        match self {
            Self::Disabled => {
                let _ = state;
            }
            #[cfg(feature = "device")]
            Self::Native(native) => native.sync(state.target()),
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
