use crate::system_media::{Controls, Create, Metadata};
use hito_playback::{MediaCommand, MediaCommandSender};
use souvlaki::{
    MediaControlEvent, MediaControls, MediaMetadata, MediaPlayback, MediaPosition, PlatformConfig,
    SeekDirection,
};
use std::{
    sync::{Arc, OnceLock},
    time::Duration,
};

const SKIP: u64 = 10_000;

/// Native window handle for platforms that bind media controls to a window.
pub type WindowHandle = Arc<OnceLock<usize>>;

/// Now Playing and remote commands through the operating system: macOS
/// MPNowPlayingInfoCenter, Windows SystemMediaTransportControls, and MPRIS on
/// Linux.
pub struct OsControls(MediaControls);

impl OsControls {
    pub fn create(commands: &MediaCommandSender, window: &WindowHandle) -> Create<Self> {
        let hwnd = window.get().map(|handle| *handle as *mut std::ffi::c_void);
        if cfg!(windows) && hwnd.is_none() {
            return Create::Later;
        }
        let Ok(mut controls) = MediaControls::new(PlatformConfig {
            dbus_name: "hito",
            display_name: "Hito",
            hwnd,
        }) else {
            return Create::Failed;
        };
        let commands = commands.clone();
        let attached = controls
            .attach(move |event| {
                if let Some(command) = command(event)
                    && commands.try_send(command).is_err()
                {
                    tracing::debug!(
                        event = "desktop_system_media_command_dropped",
                        "System media command dropped"
                    );
                }
            })
            .is_ok();
        if attached {
            Create::Ready(Self(controls))
        } else {
            Create::Failed
        }
    }
}

pub(crate) fn command(event: MediaControlEvent) -> Option<MediaCommand> {
    let forward = |direction| matches!(direction, SeekDirection::Forward);
    Some(match event {
        MediaControlEvent::Play => MediaCommand::Play,
        MediaControlEvent::Pause => MediaCommand::Pause,
        MediaControlEvent::Toggle => MediaCommand::Toggle,
        MediaControlEvent::Next => MediaCommand::Next,
        MediaControlEvent::Previous => MediaCommand::Previous,
        MediaControlEvent::Stop => MediaCommand::Stop,
        MediaControlEvent::Seek(direction) => MediaCommand::SeekBy {
            forward: forward(direction),
            delta_ms: SKIP,
        },
        MediaControlEvent::SeekBy(direction, amount) => MediaCommand::SeekBy {
            forward: forward(direction),
            delta_ms: amount.as_millis().min(u64::MAX as u128) as u64,
        },
        MediaControlEvent::SetPosition(MediaPosition(position)) => MediaCommand::Seek {
            position_ms: position.as_millis().min(u64::MAX as u128) as u64,
        },
        MediaControlEvent::SetVolume(_)
        | MediaControlEvent::OpenUri(_)
        | MediaControlEvent::Raise
        | MediaControlEvent::Quit => return None,
    })
}

impl Controls for OsControls {
    fn metadata(&mut self, metadata: &Metadata<'_>) {
        let result = self.0.set_metadata(MediaMetadata {
            title: Some(metadata.title),
            artist: metadata.artist,
            album: metadata.album,
            cover_url: metadata.cover_url,
            duration: metadata.duration,
        });
        log_failure(result.is_err());
    }

    fn playback(&mut self, playing: bool, progress: Duration) {
        let progress = Some(MediaPosition(progress));
        let result = self.0.set_playback(if playing {
            MediaPlayback::Playing { progress }
        } else {
            MediaPlayback::Paused { progress }
        });
        log_failure(result.is_err());
    }

    fn reset(&mut self) {
        let metadata = self.0.set_metadata(MediaMetadata::default());
        let playback = self.0.set_playback(MediaPlayback::Stopped);
        log_failure(metadata.is_err() || playback.is_err());
    }
}

fn log_failure(failed: bool) {
    if failed {
        tracing::debug!(
            event = "desktop_system_media_update_failed",
            "Operating-system media update failed"
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn system_events_become_transport_commands_with_absolute_and_relative_seeks() {
        assert_eq!(
            command(MediaControlEvent::Toggle),
            Some(MediaCommand::Toggle)
        );
        assert_eq!(command(MediaControlEvent::Stop), Some(MediaCommand::Stop));
        assert_eq!(
            command(MediaControlEvent::SetPosition(MediaPosition(
                Duration::from_millis(90_500)
            ))),
            Some(MediaCommand::Seek {
                position_ms: 90_500
            })
        );
        assert_eq!(
            command(MediaControlEvent::SeekBy(
                SeekDirection::Backward,
                Duration::from_secs(5)
            )),
            Some(MediaCommand::SeekBy {
                forward: false,
                delta_ms: 5000
            })
        );
        assert_eq!(
            command(MediaControlEvent::Seek(SeekDirection::Forward)),
            Some(MediaCommand::SeekBy {
                forward: true,
                delta_ms: SKIP
            })
        );
        assert_eq!(command(MediaControlEvent::Quit), None);
        assert_eq!(command(MediaControlEvent::OpenUri("x".into())), None);
    }
}
