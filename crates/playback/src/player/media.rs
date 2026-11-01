use super::{Action, PlayerAction, PlayerCommand, PlayerError, Request, ownership::State};
use crate::media::{
    MediaCommand, MediaIntegration, artwork,
    publisher::{Loaded, Observation, Publisher, TrackKey},
};
use hito_core::{Actor, Catalog};
use std::{
    future::Future,
    pin::Pin,
    time::{Duration, Instant},
};
use tokio::sync::{mpsc, oneshot};
use uuid::Uuid;

const ARTWORK_TIMEOUT: Duration = Duration::from_secs(3);
const MIN_COMMAND_GAP: Duration = Duration::from_millis(50);
const RETRY_DELAYS: [Duration; 5] = [
    Duration::from_millis(500),
    Duration::from_secs(1),
    Duration::from_secs(2),
    Duration::from_secs(4),
    Duration::from_secs(8),
];
pub(super) const RESERVED_CLIENT_SLOTS: usize = 16;

enum Fetched {
    Found(Loaded, Option<String>),
    Gone,
    Failed,
}
type TextLoading = Pin<Box<dyn Future<Output = (TrackKey, Fetched)> + Send>>;
type ArtworkLoading =
    Pin<Box<dyn Future<Output = (TrackKey, Option<artwork::MediaArtwork>)> + Send>>;

struct Target {
    actor: Actor,
    attempt: usize,
}

pub(super) struct Driver {
    publisher: Publisher,
    commands: Option<mpsc::Receiver<MediaCommand>>,
    catalog: Catalog,
    target: Option<Target>,
    text: Option<TextLoading>,
    artwork: Option<ArtworkLoading>,
    pending: Option<MediaCommand>,
    last_dispatch: Instant,
}

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

pub(super) fn retry_delay(attempt: usize) -> Option<Duration> {
    RETRY_DELAYS.get(attempt).copied()
}

impl Driver {
    pub fn new(integration: MediaIntegration, catalog: Catalog) -> Self {
        Self {
            publisher: Publisher::new(integration.sink),
            commands: Some(integration.commands),
            catalog,
            target: None,
            text: None,
            artwork: None,
            pending: None,
            last_dispatch: Instant::now() - MIN_COMMAND_GAP,
        }
    }

    pub fn duration_ms(&self) -> Option<u64> {
        self.publisher.duration_ms()
    }

    pub fn sync(&mut self, state: &State) {
        let Some(owned) = state.owner.as_ref() else {
            self.drop_track();
            return;
        };
        let snapshot = owned.queue.snapshot();
        let Some(current) = snapshot.current else {
            self.drop_track();
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
            self.artwork = None;
            self.text = Some(Box::pin(fetch_text(
                self.catalog.clone(),
                owned.actor.clone(),
                key.clone(),
                Duration::ZERO,
            )));
            self.target = Some(Target {
                actor: owned.actor.clone(),
                attempt: 0,
            });
        }
    }

    fn drop_track(&mut self) {
        self.publisher.observe(None);
        self.target = None;
        self.text = None;
        self.artwork = None;
    }

    pub async fn next(&mut self) -> MediaCommand {
        loop {
            tokio::select! {
                biased;
                (key, fetched) = async { self.text.as_mut().unwrap().await }, if self.text.is_some() => {
                    self.text = None;
                    self.finish_text(key, fetched);
                },
                (key, artwork) = async { self.artwork.as_mut().unwrap().await }, if self.artwork.is_some() => {
                    self.artwork = None;
                    self.publisher.artwork(&key, artwork);
                },
                _ = tokio::time::sleep_until((self.last_dispatch + MIN_COMMAND_GAP).into()), if self.pending.is_some() => {
                    self.last_dispatch = Instant::now();
                    return self.pending.take().unwrap();
                },
                command = async { self.commands.as_mut().unwrap().recv().await }, if self.commands.is_some() => {
                    match command {
                        Some(command) => self.pending = Some(command),
                        None => self.commands = None,
                    }
                },
                else => std::future::pending::<()>().await,
            }
        }
    }

    fn finish_text(&mut self, key: TrackKey, fetched: Fetched) {
        if !self.publisher.is_current(&key) {
            return;
        }
        let Some(target) = self.target.as_mut() else {
            return;
        };
        match fetched {
            Fetched::Found(loaded, release) => {
                self.publisher.loaded(&key, Some(loaded));
                self.artwork = Some(Box::pin(fetch_artwork(
                    self.catalog.clone(),
                    target.actor.clone(),
                    key,
                    release,
                )));
            }
            Fetched::Gone => self.publisher.loaded(&key, None),
            Fetched::Failed => {
                let Some(delay) = retry_delay(target.attempt) else {
                    self.publisher.loaded(&key, None);
                    return;
                };
                target.attempt += 1;
                self.text = Some(Box::pin(fetch_text(
                    self.catalog.clone(),
                    target.actor.clone(),
                    key,
                    delay,
                )));
            }
        }
    }

    pub fn close(&mut self) {
        self.drop_track();
        self.pending = None;
    }
}

impl State {
    pub(super) fn resolve_host(
        &self,
        actor: &Actor,
        command: MediaCommand,
        duration_ms: Option<u64>,
    ) -> Result<PlayerCommand, PlayerError> {
        let owned = self
            .owner
            .as_ref()
            .filter(|owned| owned.actor.same_session(actor))
            .ok_or(PlayerError::Conflict)?;
        let view = View {
            has_current: owned.queue.snapshot().current.is_some(),
            desired_playing: owned.desired_playing,
            position_ms: owned.position.borrow().position_ms,
            duration_ms,
        };
        let action = action(command, &view).ok_or(PlayerError::Invalid)?;
        let command = PlayerCommand {
            id: Uuid::new_v4(),
            lease: owned.lease,
            sequence: owned.sequence + 1,
            revision: owned.queue.revision(),
            action,
        };
        self.preflight(actor, &Action::Submit(command.clone()))?;
        Ok(command)
    }
}

pub(super) fn host_request(
    actor: Actor,
    command: MediaCommand,
    duration_ms: Option<u64>,
) -> Request {
    let (reply, _) = oneshot::channel();
    Request {
        actor,
        action: Action::Host {
            command,
            duration_ms,
        },
        reply,
        span: tracing::Span::current(),
        started: Instant::now(),
    }
}

async fn fetch_text(
    catalog: Catalog,
    actor: Actor,
    key: TrackKey,
    delay: Duration,
) -> (TrackKey, Fetched) {
    if !delay.is_zero() {
        tokio::time::sleep(delay).await;
    }
    let fetched = match catalog.recordings_by_ids(&actor, &[key.recording]).await {
        Ok(recordings) => match recordings.into_iter().next() {
            Some(recording) => Fetched::Found(
                Loaded {
                    title: recording.title,
                    artist: recording.artist,
                    album: recording.album.unwrap_or_default(),
                    server_duration_ms: recording.duration_ms.max(0) as u64,
                },
                recording.release_id,
            ),
            None => Fetched::Gone,
        },
        Err(error) => {
            tracing::debug!(
                event = "media_metadata_unavailable",
                error_kind = error.code(),
                "System media metadata unavailable"
            );
            match error {
                hito_core::Error::NotFound
                | hito_core::Error::Forbidden
                | hito_core::Error::Unauthorized => Fetched::Gone,
                _ => Fetched::Failed,
            }
        }
    };
    (key, fetched)
}

async fn fetch_artwork(
    catalog: Catalog,
    actor: Actor,
    key: TrackKey,
    release: Option<String>,
) -> (TrackKey, Option<artwork::MediaArtwork>) {
    let found = tokio::time::timeout(ARTWORK_TIMEOUT, async {
        if let Some(release) = &release
            && let Ok(Some(found)) = catalog.release_artwork(&actor, release).await
            && let Some(bounded) = artwork::bound(found.bytes)
        {
            return Some(bounded);
        }
        let found = catalog
            .artwork(&actor, &key.recording.to_string())
            .await
            .ok()??;
        artwork::bound(found.bytes)
    })
    .await
    .unwrap_or(None);
    (key, found)
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
    fn metadata_loads_retry_with_growing_bounded_delays() {
        let delays: Vec<_> = (0..6).map(retry_delay).collect();
        assert_eq!(delays[0], Some(Duration::from_millis(500)));
        assert!(delays[..5].windows(2).all(|pair| pair[0] < pair[1]));
        assert_eq!(delays[5], None);
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
