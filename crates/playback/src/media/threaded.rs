use super::{MediaPlayback, MediaSink, MediaTrack};
use std::sync::{Arc, Condvar, Mutex, MutexGuard};

#[derive(Default)]
struct Pending {
    clear: bool,
    track: Option<MediaTrack>,
    playback: Option<MediaPlayback>,
    closed: bool,
}

#[derive(Default)]
struct Shared {
    pending: Mutex<Pending>,
    wake: Condvar,
}

impl Shared {
    fn lock(&self) -> MutexGuard<'_, Pending> {
        self.pending
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

pub(crate) struct ThreadedSink {
    shared: Arc<Shared>,
}

impl ThreadedSink {
    pub fn spawn(sink: impl MediaSink) -> Self {
        let shared = Arc::new(Shared::default());
        let worker = shared.clone();
        let started = std::thread::Builder::new()
            .name("hito-system-media".into())
            .spawn(move || run(sink, &worker));
        if started.is_err() {
            tracing::warn!(
                event = "media_sink_thread_failed",
                "System media thread could not start"
            );
            shared.lock().closed = true;
        }
        Self { shared }
    }

    fn update(&self, change: impl FnOnce(&mut Pending)) {
        let mut pending = self.shared.lock();
        if !pending.closed {
            change(&mut pending);
            self.shared.wake.notify_one();
        }
    }
}

impl MediaSink for ThreadedSink {
    fn set_track(&mut self, track: &MediaTrack) {
        self.update(|pending| pending.track = Some(track.clone()));
    }
    fn set_playback(&mut self, playback: MediaPlayback) {
        self.update(|pending| pending.playback = Some(playback));
    }
    fn clear(&mut self) {
        self.update(|pending| {
            pending.clear = true;
            pending.track = None;
            pending.playback = None;
        });
    }
}

impl Drop for ThreadedSink {
    fn drop(&mut self) {
        let mut pending = self.shared.lock();
        pending.clear = true;
        pending.track = None;
        pending.playback = None;
        pending.closed = true;
        self.shared.wake.notify_one();
    }
}

fn run(mut sink: impl MediaSink, shared: &Shared) {
    loop {
        let (clear, track, playback, closed) = {
            let mut pending = shared.lock();
            while !(pending.clear || pending.track.is_some() || pending.playback.is_some())
                && !pending.closed
            {
                pending = shared
                    .wake
                    .wait(pending)
                    .unwrap_or_else(|poisoned| poisoned.into_inner());
            }
            (
                std::mem::take(&mut pending.clear),
                pending.track.take(),
                pending.playback.take(),
                pending.closed,
            )
        };
        if clear {
            sink.clear();
        }
        if let Some(track) = track {
            sink.set_track(&track);
        }
        if let Some(playback) = playback {
            sink.set_playback(playback);
        }
        if closed {
            return;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{sync::mpsc, time::Duration};

    #[derive(Debug, PartialEq)]
    enum Call {
        Track(String),
        Playback(u64),
        Clear,
    }
    struct Gated {
        gate: mpsc::Receiver<()>,
        calls: mpsc::Sender<Call>,
    }
    impl MediaSink for Gated {
        fn set_track(&mut self, track: &MediaTrack) {
            let _ = self.gate.recv();
            let _ = self.calls.send(Call::Track(track.title.clone()));
        }
        fn set_playback(&mut self, playback: MediaPlayback) {
            if let MediaPlayback::Paused { position_ms } = playback {
                let _ = self.calls.send(Call::Playback(position_ms));
            }
        }
        fn clear(&mut self) {
            let _ = self.calls.send(Call::Clear);
        }
    }
    fn track(title: &str) -> MediaTrack {
        MediaTrack {
            title: title.into(),
            artist: String::new(),
            album: String::new(),
            duration_ms: None,
            artwork: None,
        }
    }

    #[test]
    fn a_blocked_sink_never_blocks_callers_and_only_the_latest_state_is_delivered() {
        let (release, gate) = mpsc::channel();
        let (calls, seen) = mpsc::channel();
        let mut sink = ThreadedSink::spawn(Gated { gate, calls });
        sink.set_track(&track("one"));
        std::thread::sleep(Duration::from_millis(50));
        sink.set_track(&track("two"));
        sink.set_playback(MediaPlayback::Paused { position_ms: 1 });
        sink.set_track(&track("three"));
        sink.set_playback(MediaPlayback::Paused { position_ms: 2 });
        release.send(()).unwrap();
        release.send(()).unwrap();
        let got: Vec<Call> = (0..3)
            .map(|_| seen.recv_timeout(Duration::from_secs(2)).unwrap())
            .collect();
        assert_eq!(
            got,
            vec![
                Call::Track("one".into()),
                Call::Track("three".into()),
                Call::Playback(2)
            ]
        );
    }

    #[test]
    fn clear_discards_undelivered_state_and_drop_clears_once() {
        let (release, gate) = mpsc::channel();
        let (calls, seen) = mpsc::channel();
        let mut sink = ThreadedSink::spawn(Gated { gate, calls });
        sink.set_track(&track("one"));
        std::thread::sleep(Duration::from_millis(50));
        sink.set_track(&track("two"));
        sink.clear();
        release.send(()).unwrap();
        assert_eq!(
            seen.recv_timeout(Duration::from_secs(2)).unwrap(),
            Call::Track("one".into())
        );
        assert_eq!(
            seen.recv_timeout(Duration::from_secs(2)).unwrap(),
            Call::Clear
        );
        drop(sink);
        assert_eq!(
            seen.recv_timeout(Duration::from_secs(2)).unwrap(),
            Call::Clear
        );
        assert!(seen.recv_timeout(Duration::from_millis(200)).is_err());
    }
}
