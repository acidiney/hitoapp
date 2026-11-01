use crate::{
    AudioFormat, Decoder, Error, Input, Result, Snapshot, SpeedCapability,
    state::State,
    worker::{self, Control},
};
use std::{
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
        mpsc::{self, SyncSender},
    },
    thread::{self, JoinHandle},
    time::Duration,
};

static NEXT_ID: AtomicU64 = AtomicU64::new(1);

#[derive(Clone)]
pub struct Stop {
    id: u64,
    state: Arc<State>,
    commands: SyncSender<Control>,
}

impl Stop {
    #[tracing::instrument(level = "debug", skip_all, fields(audio_id = self.id))]
    pub fn request(&self) {
        if self.state.stopped.swap(true, Ordering::AcqRel) {
            return;
        }
        self.state.playing.store(false, Ordering::Release);
        let _ = self.commands.try_send(Control::Stop);
        tracing::debug!(
            event = "audio_stop_requested",
            "Audio output stop requested"
        );
    }
}

pub struct Player {
    id: u64,
    format: AudioFormat,
    output: AudioFormat,
    state: Arc<State>,
    commands: SyncSender<Control>,
    worker: Option<JoinHandle<Result<()>>>,
}

impl Player {
    #[tracing::instrument(level = "info", skip_all, err(Debug), fields(audio_id))]
    pub fn open(input: impl Input + 'static) -> Result<Self> {
        #[cfg(feature = "test-support")]
        let rates = crate::test_support::restricted_output_rates();
        #[cfg(not(feature = "test-support"))]
        let rates = None;
        Self::start(input, rates)
    }

    /// Opens a player whose device offers only the listed sample rates.
    #[cfg(feature = "test-support")]
    pub fn open_with_output_rates(input: impl Input + 'static, rates: &[u32]) -> Result<Self> {
        Self::start(input, Some(rates.to_vec()))
    }

    fn start(input: impl Input + 'static, rates: Option<Vec<u32>>) -> Result<Self> {
        let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
        tracing::Span::current().record("audio_id", id);
        let decoder = Decoder::open(input)?;
        let format = decoder.format();
        let state = Arc::new(State::new());
        let (commands, receiver) = mpsc::sync_channel(1);
        let (ready, started) = mpsc::sync_channel(1);
        let worker_state = state.clone();
        let span = tracing::debug_span!("audio_worker", audio_id = id);
        let dispatch = tracing::dispatcher::get_default(Clone::clone);
        let worker = thread::Builder::new().name("hito-audio".into()).spawn(move || {
            tracing::dispatcher::with_default(&dispatch, || {
            let _entered = span.enter();
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                worker::run(decoder, receiver, worker_state.clone(), ready.clone(), rates)
            })).unwrap_or(Err(Error::Worker));
            if let Err(error) = result {
                worker_state.fail(error);
                let _ = ready.try_send(Err(error));
                tracing::warn!(event = "audio_worker_failed", error_kind = ?error, "Audio worker failed");
            }
            result
            })
        }).map_err(|_| Error::Worker)?;
        let output = match started.recv().unwrap_or(Err(Error::Worker)) {
            Ok(output) => output,
            Err(error) => {
                state.stopped.store(true, Ordering::Release);
                let _ = commands.try_send(Control::Stop);
                let _ = worker.join();
                return Err(error);
            }
        };
        Ok(Self {
            id,
            format,
            output,
            state,
            commands,
            worker: Some(worker),
        })
    }

    /// The representation presented to the output device after negotiation.
    pub fn output_format(&self) -> AudioFormat {
        self.output
    }

    pub fn speed_capability(&self) -> SpeedCapability {
        self.format.speed_capability()
    }

    pub fn snapshot(&self) -> Snapshot {
        self.state.snapshot(self.format)
    }

    pub fn stop_handle(&self) -> Stop {
        Stop {
            id: self.id,
            state: self.state.clone(),
            commands: self.commands.clone(),
        }
    }

    #[tracing::instrument(level = "debug", skip_all, err(Debug), fields(audio_id = self.id))]
    pub fn resume(&mut self) -> Result<()> {
        if self.state.stopped.load(Ordering::Acquire) {
            return Err(Error::Interrupted);
        }
        if let Some(error) = self.snapshot().error {
            return Err(error);
        }
        self.state.playing.store(true, Ordering::Release);
        let _ = self.commands.try_send(Control::Wake);
        tracing::debug!(event = "audio_resumed", "Audio playback resumed");
        Ok(())
    }

    #[tracing::instrument(level = "debug", skip_all, fields(audio_id = self.id))]
    pub fn pause(&mut self) {
        self.state.playing.store(false, Ordering::Release);
        tracing::debug!(event = "audio_paused", "Audio playback paused");
    }

    #[tracing::instrument(level = "debug", skip_all, err(Debug), fields(audio_id = self.id))]
    pub fn volume(&mut self, value: f32) -> Result<()> {
        if !value.is_finite() || !(0. ..=1.).contains(&value) {
            return Err(Error::InvalidControl);
        }
        self.state.volume.store(value.to_bits(), Ordering::Release);
        tracing::debug!(
            event = "audio_volume_changed",
            volume = value,
            "Audio volume changed"
        );
        Ok(())
    }

    #[tracing::instrument(level = "debug", skip_all, err(Debug), fields(audio_id = self.id))]
    pub fn seek(&mut self, position: Duration) -> Result<()> {
        if self.state.stopped.load(Ordering::Acquire) {
            return Err(Error::Interrupted);
        }
        let snapshot = self.snapshot();
        if let Some(error) = snapshot.error {
            return Err(error);
        }
        let position = self
            .format
            .duration
            .map_or(position, |duration| position.min(duration));
        self.state.playing.store(false, Ordering::Release);
        self.state.seek_requested.store(true, Ordering::Release);
        let (reply, applied) = mpsc::sync_channel(1);
        self.commands
            .send(Control::Seek { position, reply })
            .map_err(|_| Error::Worker)?;
        let result = applied.recv().map_err(|_| Error::Worker)?;
        if let Err(error) = result {
            self.state.fail(error);
            return Err(error);
        }
        self.state
            .playing
            .store(snapshot.playing, Ordering::Release);
        tracing::debug!(
            event = "audio_seeked",
            position_ms = position.as_millis() as u64,
            "Audio playback seeked"
        );
        Ok(())
    }

    #[tracing::instrument(level = "info", skip_all, err(Debug), fields(audio_id = self.id))]
    pub fn close(mut self) -> Result<()> {
        self.finish()
    }

    #[tracing::instrument(level = "debug", skip_all, err(Debug), fields(audio_id = self.id))]
    pub fn speed(&mut self, value: f32) -> Result<()> {
        if !value.is_finite() || !(0.5..=3.).contains(&value) {
            return Err(Error::InvalidControl);
        }
        if self.state.stopped.load(Ordering::Acquire) {
            return Err(Error::Interrupted);
        }
        if let Some(error) = self.snapshot().error {
            return Err(error);
        }
        if !self.format.speed_capability().supports(value) {
            return Err(Error::Unsupported);
        }
        let (reply, applied) = mpsc::sync_channel(1);
        self.commands
            .send(Control::Speed { value, reply })
            .map_err(|_| Error::Worker)?;
        let result = applied.recv().map_err(|_| Error::Worker)?;
        if let Err(error) = result {
            self.state.fail(error);
            return Err(error);
        }
        tracing::debug!(
            event = "audio_speed_changed",
            speed = value,
            "Audio playback speed changed"
        );
        Ok(())
    }

    #[tracing::instrument(level = "debug", skip_all, err(Debug), fields(audio_id = self.id))]
    fn finish(&mut self) -> Result<()> {
        if self.worker.is_none() {
            return Ok(());
        }
        self.stop_handle().request();
        let result = self
            .worker
            .take()
            .map_or(Ok(()), |worker| worker.join().map_err(|_| Error::Worker)?);
        tracing::info!(event = "audio_player_closed", "Audio player workers joined");
        result
    }
}

impl Drop for Player {
    fn drop(&mut self) {
        let _ = self.finish();
    }
}
