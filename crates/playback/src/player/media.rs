use super::{Action, PlayerAction, PlayerCommand, Request, ownership::State};
use crate::media::{
    MediaCommand, MediaIntegration, artwork,
    publisher::{Loaded, Observation, Publisher, TrackKey},
};
use hito_core::{Actor, Catalog};
use std::{future::Future, pin::Pin, time::Duration};
use tokio::sync::mpsc;
use uuid::Uuid;

const ARTWORK_TIMEOUT: Duration = Duration::from_secs(3);

type Loading = Pin<Box<dyn Future<Output = (TrackKey, Option<Loaded>)> + Send>>;

pub(super) enum Event {
    Loaded(TrackKey, Option<Loaded>),
    Command(MediaCommand),
}

pub(super) struct Driver {
    publisher: Publisher,
    commands: Option<mpsc::Receiver<MediaCommand>>,
    catalog: Catalog,
    loading: Option<Loading>,
}

/// What a command needs to know about the current playback.
#[derive(Debug, Clone, Copy)]
pub(super) struct View {
    pub has_current: bool,
    pub desired_playing: bool,
    pub position_ms: u64,
    pub duration_ms: Option<u64>,
}

pub(super) fn action(command: MediaCommand, view: &View) -> Option<PlayerAction> {
    if !view.has_current {
        return None;
    }
    let seek = |position_ms: u64| {
        view.duration_ms.map(|duration| PlayerAction::Seek {
            position_ms: position_ms.min(duration),
        })
    };
    match command {
        MediaCommand::Play => (!view.desired_playing).then_some(PlayerAction::Resume {}),
        MediaCommand::Pause => Some(PlayerAction::Pause {}),
        MediaCommand::Toggle if view.desired_playing => Some(PlayerAction::Pause {}),
        MediaCommand::Toggle => Some(PlayerAction::Resume {}),
        MediaCommand::Next => Some(PlayerAction::Next {}),
        MediaCommand::Previous => Some(PlayerAction::Previous {}),
        MediaCommand::Stop => Some(PlayerAction::Clear {}),
        MediaCommand::Seek { position_ms } => seek(position_ms),
        MediaCommand::SeekBy { forward, delta_ms } => seek(if forward {
            view.position_ms.saturating_add(delta_ms)
        } else {
            view.position_ms.saturating_sub(delta_ms)
        }),
    }
}

impl Driver {
    pub fn new(integration: MediaIntegration, catalog: Catalog) -> Self {
        Self {
            publisher: Publisher::new(integration.sink),
            commands: Some(integration.commands),
            catalog,
            loading: None,
        }
    }

    pub fn sync(&mut self, state: &State) {
        let Some(owned) = state.owner.as_ref() else {
            self.loading = None;
            self.publisher.observe(None);
            return;
        };
        let snapshot = owned.queue.snapshot();
        let Some(current) = snapshot.current else {
            self.loading = None;
            self.publisher.observe(None);
            return;
        };
        let position = *owned.position.borrow();
        let same_selection =
            position.entry == Some(current.id) && position.selection == snapshot.selection;
        let observation = Observation {
            account: &owned.actor.user().id,
            entry: current.id,
            recording: current.recording_id,
            playing: owned.playing,
            rate: owned.speed,
            position_ms: if same_selection {
                position.position_ms
            } else {
                0
            },
            duration_ms: if same_selection {
                position.duration_ms
            } else {
                None
            },
            selection: snapshot.selection,
            seek_id: if same_selection {
                position.seek_id
            } else {
                None
            },
        };
        if let Some(key) = self.publisher.observe(Some(&observation)) {
            self.loading = Some(Box::pin(load(
                self.catalog.clone(),
                owned.actor.clone(),
                key,
            )));
        }
    }

    pub async fn next(&mut self) -> Event {
        loop {
            tokio::select! {
                biased;
                (key, loaded) = async { self.loading.as_mut().unwrap().await }, if self.loading.is_some() => {
                    self.loading = None;
                    return Event::Loaded(key, loaded);
                },
                command = async { self.commands.as_mut().unwrap().recv().await }, if self.commands.is_some() => {
                    match command {
                        Some(command) => return Event::Command(command),
                        None => self.commands = None,
                    }
                },
                else => std::future::pending::<()>().await,
            }
        }
    }

    pub fn loaded(&mut self, key: &TrackKey, loaded: Option<Loaded>) {
        self.publisher.loaded(key, loaded);
    }

    /// Resolves a system command against the current playback. The caller
    /// submits the action as the owning account through the normal admission
    /// path, so session and ordering checks are the ones every client gets.
    pub fn request(&self, command: MediaCommand, state: &State) -> Option<(Actor, PlayerAction)> {
        let owned = state.owner.as_ref()?;
        let position = *owned.position.borrow();
        let snapshot = owned.queue.snapshot();
        let view = View {
            has_current: snapshot.current.is_some(),
            desired_playing: owned.desired_playing,
            position_ms: position.position_ms,
            duration_ms: self.publisher.duration_ms(),
        };
        action(command, &view).map(|action| (owned.actor.clone(), action))
    }

    pub fn close(&mut self) {
        self.loading = None;
        self.publisher.clear();
    }
}

async fn load(catalog: Catalog, actor: Actor, key: TrackKey) -> (TrackKey, Option<Loaded>) {
    let loaded = fetch(&catalog, &actor, &key).await;
    (key, loaded)
}

async fn fetch(catalog: &Catalog, actor: &Actor, key: &TrackKey) -> Option<Loaded> {
    let recordings = catalog
        .recordings_by_ids(actor, &[key.recording])
        .await
        .inspect_err(|error| {
            tracing::debug!(
                event = "media_metadata_unavailable",
                error_kind = error.code(),
                "System media metadata unavailable"
            );
        })
        .ok()?;
    let recording = recordings.into_iter().next()?;
    let artwork = tokio::time::timeout(ARTWORK_TIMEOUT, async {
        if let Some(release) = &recording.release_id
            && let Ok(Some(found)) = catalog.release_artwork(actor, release).await
            && let Some(bounded) = artwork::bound(found.bytes)
        {
            return Some(bounded);
        }
        let found = catalog.artwork(actor, &recording.id).await.ok()??;
        artwork::bound(found.bytes)
    })
    .await
    .unwrap_or(None);
    Some(Loaded {
        title: recording.title,
        artist: recording.artist,
        album: recording.album.unwrap_or_default(),
        server_duration_ms: recording.duration_ms.max(0) as u64,
        artwork,
    })
}

/// Fills in the ordering fields of a system command from the owner's current
/// state, so it never competes with a client's sequence numbers.
pub(super) fn stamp(state: &State, request: &mut Request) {
    let Action::Host(command) = &mut request.action else {
        return;
    };
    if let Some(owned) = state
        .owner
        .as_ref()
        .filter(|owned| owned.actor.same_session(&request.actor))
    {
        command.lease = owned.lease;
        command.sequence = owned.sequence + 1;
        command.revision = owned.queue.revision();
    }
}

pub(super) fn host_request(actor: Actor, action: PlayerAction) -> Request {
    let (reply, _) = tokio::sync::oneshot::channel();
    Request {
        actor,
        action: Action::Host(PlayerCommand {
            id: Uuid::new_v4(),
            lease: Uuid::nil(),
            sequence: 0,
            revision: 0,
            action,
        }),
        reply,
        span: tracing::Span::current(),
        started: std::time::Instant::now(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn view() -> View {
        View {
            has_current: true,
            desired_playing: true,
            position_ms: 60_000,
            duration_ms: Some(200_000),
        }
    }

    #[test]
    fn transport_commands_map_to_explicit_player_actions() {
        let mut view = view();
        assert!(matches!(
            action(MediaCommand::Pause, &view),
            Some(PlayerAction::Pause {})
        ));
        assert!(matches!(
            action(MediaCommand::Toggle, &view),
            Some(PlayerAction::Pause {})
        ));
        assert!(action(MediaCommand::Play, &view).is_none());
        view.desired_playing = false;
        assert!(matches!(
            action(MediaCommand::Play, &view),
            Some(PlayerAction::Resume {})
        ));
        assert!(matches!(
            action(MediaCommand::Toggle, &view),
            Some(PlayerAction::Resume {})
        ));
        assert!(matches!(
            action(MediaCommand::Next, &view),
            Some(PlayerAction::Next {})
        ));
        assert!(matches!(
            action(MediaCommand::Previous, &view),
            Some(PlayerAction::Previous {})
        ));
        assert!(matches!(
            action(MediaCommand::Stop, &view),
            Some(PlayerAction::Clear {})
        ));
    }

    #[test]
    fn seeks_are_absolute_clamped_to_the_reconciled_duration_and_need_one() {
        let mut view = view();
        let seek = |command, view: &View| match action(command, view) {
            Some(PlayerAction::Seek { position_ms }) => Some(position_ms),
            _ => None,
        };
        assert_eq!(
            seek(
                MediaCommand::Seek {
                    position_ms: 90_000
                },
                &view
            ),
            Some(90_000)
        );
        assert_eq!(
            seek(
                MediaCommand::Seek {
                    position_ms: 900_000
                },
                &view
            ),
            Some(200_000)
        );
        let by = |forward, delta_ms| MediaCommand::SeekBy { forward, delta_ms };
        assert_eq!(seek(by(true, 10_000), &view), Some(70_000));
        assert_eq!(seek(by(false, 10_000), &view), Some(50_000));
        assert_eq!(seek(by(false, 100_000), &view), Some(0));
        assert_eq!(seek(by(true, 500_000), &view), Some(200_000));
        view.duration_ms = None;
        assert_eq!(seek(MediaCommand::Seek { position_ms: 1 }, &view), None);
    }

    #[test]
    fn nothing_is_controlled_without_a_current_entry() {
        let mut view = view();
        view.has_current = false;
        for command in [
            MediaCommand::Play,
            MediaCommand::Pause,
            MediaCommand::Toggle,
            MediaCommand::Next,
            MediaCommand::Previous,
            MediaCommand::Stop,
            MediaCommand::Seek { position_ms: 0 },
        ] {
            assert!(action(command, &view).is_none());
        }
    }
}
