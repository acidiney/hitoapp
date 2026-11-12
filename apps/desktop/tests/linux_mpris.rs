#![cfg(all(target_os = "linux", feature = "native-media"))]

use base64::Engine;
use dbus::{
    arg::{ArgType, PropMap},
    blocking::{Connection, Proxy, stdintf::org_freedesktop_dbus::Properties},
};
use hito_desktop::{Create, OsControls, SystemMedia, WindowHandle};
use hito_playback::{
    MediaArtwork, MediaCommand, MediaPlayback, MediaSink, MediaTrack, media_commands,
};
use std::sync::Arc;
use std::{
    sync::{Mutex, mpsc},
    thread,
    time::{Duration, Instant},
};
use tokio::sync::mpsc::{Receiver, error::TryRecvError};
use uuid::Uuid;

const BUS: &str = "org.mpris.MediaPlayer2.hito";
const OBJECT: &str = "/org/mpris/MediaPlayer2";
const PLAYER: &str = "org.mpris.MediaPlayer2.Player";
static SESSION: Mutex<()> = Mutex::new(());

struct Deadline {
    until: Instant,
    done: mpsc::Sender<()>,
    watchdog: Option<thread::JoinHandle<()>>,
}

impl Deadline {
    fn new() -> Self {
        let (done, finished) = mpsc::channel();
        let watchdog = thread::spawn(move || {
            if finished.recv_timeout(Duration::from_secs(30)).is_err() {
                eprintln!("linux_mpris_test_deadline_exceeded");
                std::process::abort();
            }
        });
        Self {
            until: Instant::now() + Duration::from_secs(20),
            done,
            watchdog: Some(watchdog),
        }
    }

    fn call_budget(&self) -> Duration {
        self.until
            .checked_duration_since(Instant::now())
            .filter(|remaining| !remaining.is_zero())
            .expect("private MPRIS protocol deadline")
            .min(Duration::from_secs(1))
    }
}

impl Drop for Deadline {
    fn drop(&mut self) {
        let _ = self.done.send(());
        self.watchdog.take().unwrap().join().unwrap();
    }
}

struct Client<'a> {
    connection: Connection,
    deadline: &'a Deadline,
}

impl<'a> Client<'a> {
    fn new(deadline: &'a Deadline) -> Self {
        assert!(
            std::env::var_os("DBUS_SESSION_BUS_ADDRESS").is_some(),
            "run this test inside its owned dbus-run-session"
        );
        Self {
            connection: Connection::new_session().expect("owned session bus connection"),
            deadline,
        }
    }

    fn player(&self) -> Proxy<'_, &Connection> {
        self.connection
            .with_proxy(BUS, OBJECT, self.deadline.call_budget())
    }

    fn track(&self, expected_title: &str, duration_ms: Option<u64>) -> String {
        loop {
            if let Ok(metadata) = self.player().get::<PropMap>(PLAYER, "Metadata")
                && metadata
                    .get("xesam:title")
                    .and_then(|value| value.0.as_str())
                    == Some(expected_title)
                && metadata
                    .get("mpris:length")
                    .and_then(|value| value.0.as_i64())
                    == duration_ms.map(|value| (value * 1000) as i64)
            {
                let track = &metadata.get("mpris:trackid").expect("actual track ID").0;
                assert_eq!(track.arg_type(), ArgType::ObjectPath);
                return track.as_str().expect("track object path").to_owned();
            }
            self.deadline.call_budget();
            thread::sleep(Duration::from_millis(10));
        }
    }

    fn set_position(&self, track: &str, microseconds: i64) {
        let track = dbus::Path::new(track.to_owned()).expect("observed track object path");
        let _: () = self
            .player()
            .method_call(PLAYER, "SetPosition", (track, microseconds))
            .expect("SetPosition protocol reply");
    }

    fn pause_marker(&self) {
        let _: () = self
            .player()
            .method_call(PLAYER, "Pause", ())
            .expect("ordered supported marker reply");
    }

    fn wait_until_cleared(&self) {
        loop {
            if self
                .player()
                .get::<PropMap>(PLAYER, "Metadata")
                .is_ok_and(|metadata| metadata.is_empty())
            {
                return;
            }
            self.deadline.call_budget();
            thread::sleep(Duration::from_millis(10));
        }
    }

    fn wait_until_released(&self) {
        loop {
            let (owned,): (bool,) = self
                .connection
                .with_proxy(
                    "org.freedesktop.DBus",
                    "/org/freedesktop/DBus",
                    self.deadline.call_budget(),
                )
                .method_call("org.freedesktop.DBus", "NameHasOwner", (BUS,))
                .expect("session bus ownership reply");
            if !owned {
                return;
            }
            thread::sleep(Duration::from_millis(10));
        }
    }
}

struct Fixture<'a> {
    media: SystemMedia<OsControls>,
    commands: Receiver<MediaCommand>,
    client: Client<'a>,
    _directory: tempfile::TempDir,
}

impl<'a> Fixture<'a> {
    fn new(deadline: &'a Deadline) -> Self {
        let client = Client::new(deadline);
        let (sender, commands) = media_commands();
        let controls = match OsControls::create(&sender, &WindowHandle::default()) {
            Create::Ready(controls) => controls,
            Create::Later | Create::Failed => panic!("owned Linux controls must attach"),
        };
        drop(sender);
        let directory = tempfile::tempdir().unwrap();
        Self {
            media: SystemMedia::new(controls, directory.path().join("artwork")),
            commands,
            client,
            _directory: directory,
        }
    }

    fn publish(&mut self, title: &str) -> String {
        self.publish_metadata(&MediaTrack {
            id: Uuid::new_v4(),
            title: title.into(),
            artist: "Owned fixture artist".into(),
            album: "Owned fixture album".into(),
            duration_ms: Some(200_000),
            artwork: None,
        })
    }

    fn publish_metadata(&mut self, track: &MediaTrack) -> String {
        self.media.set_track(track);
        self.media
            .set_playback(MediaPlayback::Paused { position_ms: 0 });
        self.client.track(&track.title, track.duration_ms)
    }

    fn through_marker(&mut self) -> Vec<MediaCommand> {
        self.client.pause_marker();
        let mut observed = Vec::new();
        loop {
            match self.commands.try_recv() {
                Ok(command) => {
                    observed.push(command);
                    assert!(observed.len() <= 16, "bounded media channel");
                }
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => panic!("controls disconnected before marker"),
            }
        }
        assert_eq!(
            observed.last(),
            Some(&MediaCommand::Pause),
            "the ordered marker must reach the actual command channel"
        );
        observed
    }

    fn finish(self) {
        let Self {
            media,
            mut commands,
            client,
            _directory,
        } = self;
        drop(media);
        client.wait_until_released();
        assert_eq!(commands.try_recv(), Err(TryRecvError::Disconnected));
    }
}

#[test]
fn current_track_set_position_reaches_the_media_command_channel() {
    let _session = SESSION.lock().unwrap_or_else(|error| error.into_inner());
    let deadline = Deadline::new();
    let mut fixture = Fixture::new(&deadline);
    let current = fixture.publish("Owned current track");
    fixture.client.set_position(&current, 1_500_000);
    let observed = fixture.through_marker();
    fixture.finish();
    assert_eq!(
        observed,
        vec![
            MediaCommand::SeekTrack {
                track: Uuid::parse_str(current.strip_prefix("/org/hito/audio/track/").unwrap())
                    .unwrap(),
                position_us: 1_500_000,
            },
            MediaCommand::Pause,
        ]
    );
}

#[test]
fn a_previous_tracks_set_position_does_not_enter_the_media_command_channel() {
    let _session = SESSION.lock().unwrap_or_else(|error| error.into_inner());
    let deadline = Deadline::new();
    let mut fixture = Fixture::new(&deadline);
    let previous = fixture.publish("Owned track A");
    fixture.media.clear();
    let _current = fixture.publish("Owned track B");
    fixture.client.set_position(&previous, 2_500_000);
    let observed = fixture.through_marker();
    fixture.finish();
    assert_eq!(
        observed,
        vec![MediaCommand::Pause],
        "a seek addressed to the previous track must be ignored"
    );
}

#[test]
fn invalid_track_and_out_of_bounds_positions_do_not_reach_the_command_channel() {
    let _session = SESSION.lock().unwrap_or_else(|error| error.into_inner());
    let deadline = Deadline::new();
    let mut fixture = Fixture::new(&deadline);
    let current = fixture.publish("Owned bounded track");
    fixture
        .client
        .set_position("/org/mpris/MediaPlayer2/TrackList/NoTrack", 0);
    fixture.client.set_position(
        "/org/hito/audio/track/00000000000000000000000000000000",
        1_000_000,
    );
    fixture.client.set_position(&current, -1);
    fixture.client.set_position(&current, 200_000_001);
    fixture.client.set_position(&current, i64::MAX);
    let observed = fixture.through_marker();
    fixture.media.clear();
    fixture.client.wait_until_cleared();
    fixture.client.set_position(&current, 0);
    assert_eq!(fixture.through_marker(), vec![MediaCommand::Pause]);
    fixture.finish();
    assert_eq!(observed, vec![MediaCommand::Pause]);
}

#[test]
fn unknown_duration_rejects_position_and_metadata_updates_keep_the_track_identity() {
    let _session = SESSION.lock().unwrap_or_else(|error| error.into_inner());
    let deadline = Deadline::new();
    let mut fixture = Fixture::new(&deadline);
    let mut track = MediaTrack {
        id: Uuid::new_v4(),
        title: "Owned refining track".into(),
        artist: "Owned fixture artist".into(),
        album: String::new(),
        duration_ms: None,
        artwork: None,
    };
    let unknown = fixture.publish_metadata(&track);
    fixture.client.set_position(&unknown, 0);
    assert_eq!(fixture.through_marker(), vec![MediaCommand::Pause]);
    track.duration_ms = Some(201_000);
    track.artwork = Some(Arc::new(MediaArtwork { content_type: "image/png", bytes: base64::engine::general_purpose::STANDARD.decode("iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mP8/x8AAwMCAO+j6WQAAAAASUVORK5CYII=").unwrap() }));
    let refined = fixture.publish_metadata(&track);
    assert_eq!(unknown, refined);
    fixture.client.set_position(&refined, 201_000_000);
    let observed = fixture.through_marker();
    fixture.finish();
    assert_eq!(
        observed,
        vec![
            MediaCommand::SeekTrack {
                track: track.id,
                position_us: 201_000_000
            },
            MediaCommand::Pause
        ]
    );
}
