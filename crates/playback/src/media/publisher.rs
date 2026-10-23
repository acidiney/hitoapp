use super::{MediaArtwork, MediaPlayback, MediaSink, MediaTrack};
use std::sync::Arc;
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct TrackKey {
    pub account: String,
    pub entry: Uuid,
    pub recording: Uuid,
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct Observation<'a> {
    pub account: &'a str,
    pub entry: Uuid,
    pub recording: Uuid,
    pub playing: bool,
    pub rate: u16,
    pub position_ms: u64,
    pub duration_ms: Option<u64>,
    pub selection: u64,
    pub seek_id: Option<Uuid>,
}

pub(crate) struct Loaded {
    pub title: String,
    pub artist: String,
    pub album: String,
    pub server_duration_ms: u64,
}

/// Prefers the recording's known duration only when the decoded or estimated
/// one is more than 10 percent and more than 5 seconds longer.
pub(crate) fn reconcile(decoded_ms: Option<u64>, server_ms: u64) -> Option<u64> {
    match (decoded_ms.filter(|decoded| *decoded > 0), server_ms) {
        (Some(decoded), server) if server > 0 => {
            let overlong = decoded > server + server / 10 && decoded - server > 5_000;
            Some(if overlong { server } else { decoded })
        }
        (Some(decoded), _) => Some(decoded),
        (None, 0) => None,
        (None, server) => Some(server),
    }
}

#[derive(PartialEq, Eq)]
struct Anchor {
    playing: bool,
    rate: u16,
    selection: u64,
    seek_id: Option<Uuid>,
    position_ms: Option<u64>,
}

struct Current {
    key: TrackKey,
    loaded: Option<Loaded>,
    artwork: Option<Arc<MediaArtwork>>,
    latest: Option<LatestState>,
    shown_track: Option<MediaTrack>,
    shown_anchor: Option<Anchor>,
}

#[derive(Clone, Copy)]
struct LatestState {
    playing: bool,
    rate: u16,
    position_ms: u64,
    duration_ms: Option<u64>,
    selection: u64,
    seek_id: Option<Uuid>,
}

pub(crate) struct Publisher {
    sink: Box<dyn MediaSink>,
    current: Option<Current>,
}

impl Publisher {
    pub fn new(sink: Box<dyn MediaSink>) -> Self {
        Self {
            sink,
            current: None,
        }
    }

    /// Returns the track whose metadata must now be loaded, if it changed.
    pub fn observe(&mut self, observation: Option<&Observation>) -> Option<TrackKey> {
        let Some(observation) = observation else {
            self.clear();
            return None;
        };
        let same = self.current.as_ref().is_some_and(|current| {
            current.key.account == observation.account
                && current.key.entry == observation.entry
                && current.key.recording == observation.recording
        });
        let mut started = None;
        if !same {
            self.clear();
            let key = TrackKey {
                account: observation.account.to_owned(),
                entry: observation.entry,
                recording: observation.recording,
            };
            started = Some(key.clone());
            self.current = Some(Current {
                key,
                loaded: None,
                artwork: None,
                latest: None,
                shown_track: None,
                shown_anchor: None,
            });
        }
        if let Some(current) = &mut self.current {
            current.latest = Some(LatestState {
                playing: observation.playing,
                rate: observation.rate,
                position_ms: observation.position_ms,
                duration_ms: observation.duration_ms,
                selection: observation.selection,
                seek_id: observation.seek_id,
            });
        }
        self.publish();
        started
    }

    pub fn loaded(&mut self, key: &TrackKey, loaded: Option<Loaded>) {
        let Some(current) = self.current.as_mut().filter(|current| &current.key == key) else {
            tracing::debug!(event = "media_load_stale", "Stale media metadata ignored");
            return;
        };
        current.loaded = loaded;
        self.publish();
    }

    pub fn artwork(&mut self, key: &TrackKey, artwork: Option<MediaArtwork>) {
        let Some(current) = self.current.as_mut().filter(|current| &current.key == key) else {
            tracing::debug!(event = "media_load_stale", "Stale media artwork ignored");
            return;
        };
        current.artwork = artwork.map(Arc::new);
        self.publish();
    }

    pub fn is_current(&self, key: &TrackKey) -> bool {
        self.current
            .as_ref()
            .is_some_and(|current| &current.key == key)
    }

    pub fn duration_ms(&self) -> Option<u64> {
        let current = self.current.as_ref()?;
        let loaded = current.loaded.as_ref()?;
        reconcile(
            current.latest.and_then(|latest| latest.duration_ms),
            loaded.server_duration_ms,
        )
    }

    pub fn clear(&mut self) {
        if self
            .current
            .take()
            .is_some_and(|current| current.shown_track.is_some())
        {
            self.sink.clear();
            tracing::debug!(event = "media_cleared", "System media state cleared");
        }
    }

    fn publish(&mut self) {
        let duration = self.duration_ms();
        let Some(current) = &mut self.current else {
            return;
        };
        let (Some(loaded), Some(latest)) = (&current.loaded, current.latest) else {
            return;
        };
        let unchanged = current.shown_track.as_ref().is_some_and(|shown| {
            shown.duration_ms.map(|ms| ms / 1000) == duration.map(|ms| ms / 1000)
                && match (&shown.artwork, &current.artwork) {
                    (Some(shown), Some(current)) => Arc::ptr_eq(shown, current),
                    (None, None) => true,
                    _ => false,
                }
        });
        if !unchanged {
            let track = MediaTrack {
                title: loaded.title.clone(),
                artist: loaded.artist.clone(),
                album: loaded.album.clone(),
                duration_ms: duration,
                artwork: current.artwork.clone(),
            };
            self.sink.set_track(&track);
            current.shown_track = Some(track);
            tracing::debug!(
                event = "media_track_published",
                duration_ms = duration,
                "System media track published"
            );
        }
        let anchor = Anchor {
            playing: latest.playing,
            rate: latest.rate,
            selection: latest.selection,
            seek_id: latest.seek_id,
            position_ms: (!latest.playing).then_some(latest.position_ms),
        };
        if current.shown_anchor.as_ref() != Some(&anchor) {
            self.sink.set_playback(if latest.playing {
                MediaPlayback::Playing {
                    position_ms: latest.position_ms,
                    rate: latest.rate,
                }
            } else {
                MediaPlayback::Paused {
                    position_ms: latest.position_ms,
                }
            });
            current.shown_anchor = Some(anchor);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};

    #[derive(Debug, Clone, PartialEq)]
    enum Event {
        Track(MediaTrack),
        Playback(MediaPlayback),
        Clear,
    }
    #[derive(Clone, Default)]
    struct Fake(Arc<Mutex<Vec<Event>>>);
    impl MediaSink for Fake {
        fn set_track(&mut self, track: &MediaTrack) {
            self.0.lock().unwrap().push(Event::Track(track.clone()));
        }
        fn set_playback(&mut self, playback: MediaPlayback) {
            self.0.lock().unwrap().push(Event::Playback(playback));
        }
        fn clear(&mut self) {
            self.0.lock().unwrap().push(Event::Clear);
        }
    }
    impl Fake {
        fn take(&self) -> Vec<Event> {
            std::mem::take(&mut *self.0.lock().unwrap())
        }
    }

    const ENTRY: Uuid = Uuid::from_u128(1);
    const OTHER: Uuid = Uuid::from_u128(2);
    fn obs(account: &str, entry: Uuid) -> Observation<'_> {
        Observation {
            account,
            entry,
            recording: entry,
            playing: true,
            rate: 1000,
            position_ms: 1000,
            duration_ms: None,
            selection: 1,
            seek_id: None,
        }
    }
    fn loaded(title: &str, server_duration_ms: u64) -> Loaded {
        Loaded {
            title: title.into(),
            artist: "Artist".into(),
            album: "".into(),
            server_duration_ms,
        }
    }
    fn track(title: &str, duration_ms: Option<u64>) -> Event {
        Event::Track(MediaTrack {
            title: title.into(),
            artist: "Artist".into(),
            album: "".into(),
            duration_ms,
            artwork: None,
        })
    }
    fn setup() -> (Publisher, Fake) {
        let fake = Fake::default();
        (Publisher::new(Box::new(fake.clone())), fake)
    }
    fn begin(publisher: &mut Publisher, observation: &Observation, title: &str, server: u64) {
        let key = publisher.observe(Some(observation)).unwrap();
        publisher.loaded(&key, Some(loaded(title, server)));
    }

    #[test]
    fn duration_reconciliation_prefers_the_known_duration_only_for_overlong_estimates() {
        assert_eq!(reconcile(Some(400_000), 220_000), Some(220_000));
        assert_eq!(reconcile(Some(250_000), 220_000), Some(220_000));
        assert_eq!(reconcile(Some(230_000), 220_000), Some(230_000));
        assert_eq!(reconcile(Some(12_000), 8_000), Some(12_000));
        assert_eq!(reconcile(Some(100_000), 220_000), Some(100_000));
        assert_eq!(reconcile(None, 220_000), Some(220_000));
        assert_eq!(reconcile(Some(300_000), 0), Some(300_000));
        assert_eq!(reconcile(None, 0), None);
    }

    #[test]
    fn a_track_change_publishes_metadata_once_and_position_ticks_publish_nothing() {
        let (mut publisher, fake) = setup();
        let mut observation = obs("a", ENTRY);
        assert!(publisher.observe(Some(&observation)).is_some());
        assert!(fake.take().is_empty());
        let key = TrackKey {
            account: "a".into(),
            entry: ENTRY,
            recording: ENTRY,
        };
        publisher.loaded(&key, Some(loaded("Song", 200_000)));
        assert_eq!(
            fake.take(),
            vec![
                track("Song", Some(200_000)),
                Event::Playback(MediaPlayback::Playing {
                    position_ms: 1000,
                    rate: 1000
                })
            ]
        );
        for position in [1250, 1500, 1750, 2000] {
            observation.position_ms = position;
            assert!(publisher.observe(Some(&observation)).is_none());
        }
        assert!(fake.take().is_empty());
    }

    #[test]
    fn pause_and_resume_publish_state_with_the_anchor_position() {
        let (mut publisher, fake) = setup();
        let mut observation = obs("a", ENTRY);
        begin(&mut publisher, &observation, "Song", 200_000);
        fake.take();
        observation.playing = false;
        observation.position_ms = 5000;
        publisher.observe(Some(&observation));
        assert_eq!(
            fake.take(),
            vec![Event::Playback(MediaPlayback::Paused { position_ms: 5000 })]
        );
        publisher.observe(Some(&observation));
        assert!(fake.take().is_empty());
        observation.playing = true;
        publisher.observe(Some(&observation));
        assert_eq!(
            fake.take(),
            vec![Event::Playback(MediaPlayback::Playing {
                position_ms: 5000,
                rate: 1000
            })]
        );
    }

    #[test]
    fn seeks_and_speed_changes_republish_the_anchor() {
        let (mut publisher, fake) = setup();
        let mut observation = obs("a", ENTRY);
        begin(&mut publisher, &observation, "Song", 200_000);
        fake.take();
        observation.position_ms = 90_000;
        observation.seek_id = Some(Uuid::from_u128(9));
        publisher.observe(Some(&observation));
        assert_eq!(
            fake.take(),
            vec![Event::Playback(MediaPlayback::Playing {
                position_ms: 90_000,
                rate: 1000
            })]
        );
        observation.rate = 1500;
        publisher.observe(Some(&observation));
        assert_eq!(
            fake.take(),
            vec![Event::Playback(MediaPlayback::Playing {
                position_ms: 90_000,
                rate: 1500
            })]
        );
        observation.selection = 2;
        publisher.observe(Some(&observation));
        assert_eq!(fake.take().len(), 1);
    }

    #[test]
    fn overlong_decoded_durations_use_the_known_duration_and_late_decodes_republish() {
        let (mut publisher, fake) = setup();
        let mut observation = obs("a", ENTRY);
        begin(&mut publisher, &observation, "Song", 220_000);
        fake.take();
        observation.duration_ms = Some(440_000);
        publisher.observe(Some(&observation));
        assert!(fake.take().is_empty());
        assert_eq!(publisher.duration_ms(), Some(220_000));
        observation.duration_ms = Some(215_300);
        publisher.observe(Some(&observation));
        assert_eq!(fake.take(), vec![track("Song", Some(215_300))]);
        assert_eq!(publisher.duration_ms(), Some(215_300));
    }

    #[test]
    fn a_stale_load_after_a_track_change_is_ignored() {
        let (mut publisher, fake) = setup();
        let first = publisher.observe(Some(&obs("a", ENTRY))).unwrap();
        let second = publisher.observe(Some(&obs("a", OTHER))).unwrap();
        assert!(fake.take().is_empty());
        publisher.loaded(&first, Some(loaded("Old", 1000)));
        assert!(fake.take().is_empty());
        publisher.loaded(&second, Some(loaded("New", 1000)));
        assert!(matches!(&fake.take()[0], Event::Track(t) if t.title == "New"));
    }

    #[test]
    fn leaving_a_track_or_account_clears_once_and_never_leaves_the_old_title() {
        let (mut publisher, fake) = setup();
        begin(&mut publisher, &obs("a", ENTRY), "Song", 1000);
        fake.take();
        assert!(publisher.observe(Some(&obs("b", ENTRY))).is_some());
        assert_eq!(fake.take(), vec![Event::Clear]);
        publisher.observe(None);
        publisher.observe(None);
        assert!(fake.take().is_empty());
        begin(&mut publisher, &obs("b", ENTRY), "Song", 1000);
        fake.take();
        publisher.observe(None);
        assert_eq!(fake.take(), vec![Event::Clear]);
        publisher.observe(None);
        assert!(fake.take().is_empty());
        assert_eq!(publisher.duration_ms(), None);
    }

    fn art(tag: u8) -> MediaArtwork {
        MediaArtwork {
            content_type: "image/png",
            bytes: vec![tag],
        }
    }
    fn key(entry: Uuid) -> TrackKey {
        TrackKey {
            account: "a".into(),
            entry,
            recording: entry,
        }
    }

    #[test]
    fn text_is_published_before_artwork_and_artwork_adds_one_update() {
        let (mut publisher, fake) = setup();
        begin(&mut publisher, &obs("a", ENTRY), "Song", 200_000);
        assert_eq!(fake.take().len(), 2);
        publisher.artwork(&key(ENTRY), Some(art(1)));
        let events = fake.take();
        assert_eq!(events.len(), 1);
        assert!(matches!(&events[0], Event::Track(t) if t.title == "Song" && t.artwork.is_some()));
        publisher.artwork(&key(ENTRY), Some(art(1)));
        assert!(fake.take().len() <= 1);
    }

    #[test]
    fn a_duration_only_republish_keeps_the_same_artwork_allocation() {
        let (mut publisher, fake) = setup();
        let mut observation = obs("a", ENTRY);
        begin(&mut publisher, &observation, "Song", 220_000);
        publisher.artwork(&key(ENTRY), Some(art(1)));
        let first = fake.take().into_iter().find_map(|e| match e {
            Event::Track(t) if t.artwork.is_some() => t.artwork,
            _ => None,
        });
        observation.duration_ms = Some(215_300);
        publisher.observe(Some(&observation));
        let Event::Track(second) = fake.take().remove(0) else {
            panic!("track")
        };
        assert!(Arc::ptr_eq(&first.unwrap(), &second.artwork.unwrap()));
    }

    #[test]
    fn artwork_for_a_previous_track_or_after_a_clear_is_ignored() {
        let (mut publisher, fake) = setup();
        begin(&mut publisher, &obs("a", ENTRY), "Song", 1000);
        publisher.observe(Some(&obs("a", OTHER)));
        fake.take();
        publisher.artwork(&key(ENTRY), Some(art(1)));
        assert!(fake.take().is_empty());
        assert!(publisher.is_current(&key(OTHER)));
        assert!(!publisher.is_current(&key(ENTRY)));
        publisher.observe(None);
        assert!(!publisher.is_current(&key(OTHER)));
    }

    #[test]
    fn a_failed_load_publishes_nothing_and_clear_is_idempotent() {
        let (mut publisher, fake) = setup();
        let observation = obs("a", ENTRY);
        let key = publisher.observe(Some(&observation)).unwrap();
        publisher.loaded(&key, None);
        publisher.observe(Some(&observation));
        assert!(fake.take().is_empty());
        publisher.clear();
        publisher.clear();
        assert!(fake.take().is_empty());
        begin(&mut publisher, &observation, "Song", 1000);
        fake.take();
        publisher.clear();
        publisher.clear();
        assert_eq!(fake.take(), vec![Event::Clear]);
    }
}
