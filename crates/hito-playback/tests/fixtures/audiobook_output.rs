use std::{fmt, path::Path, sync::Arc};
use tokio::sync::Notify;
use tracing::{
    Event, Subscriber,
    field::{Field, Visit},
    span::Id,
};
use tracing_subscriber::{Layer, layer::Context, registry::LookupSpan};

pub fn write_audio(path: &Path, offset: i16) -> Vec<u8> {
    let rate = 48_000_u32;
    let bytes = hito_testkit::audio::pcm16_wav(rate, 1, u64::from(rate) * 8, |index| {
        let frame = index as u32;
        let frequency = if offset != 0 {
            220.
        } else if frame < rate * 4 {
            440.
        } else if frame < rate * 4 + rate * 3 / 10 {
            880.
        } else {
            1320.
        };
        let phase = f64::from(frame) * frequency * std::f64::consts::TAU / f64::from(rate);
        (phase.sin() * 8_000.) as i16
    });
    std::fs::write(path, &bytes).unwrap();
    bytes
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ChapterStart {
    Earlier,
    Saved,
    Later,
}

pub fn first_chapter_start(samples: &[f32]) -> Option<ChapterStart> {
    samples.as_chunks::<2_400>().0.iter().find_map(|window| {
        [
            (440., ChapterStart::Earlier),
            (880., ChapterStart::Saved),
            (1320., ChapterStart::Later),
        ]
        .into_iter()
        .map(|(frequency, start)| (crate::capture::amplitude(window, frequency), start))
        .max_by(|a, b| a.0.total_cmp(&b.0))
        .filter(|(amplitude, _)| *amplitude > 0.08)
        .map(|(_, start)| start)
    })
}

#[derive(Clone, Default)]
pub struct WorkflowSignals {
    pub prepared: Arc<Notify>,
    pub admitting: Arc<Notify>,
}
#[derive(Default)]
struct Preparation(bool);
impl Visit for Preparation {
    fn record_str(&mut self, field: &Field, value: &str) {
        self.0 |= field.name() == "event" && value == "player_audiobook_prepare_started";
    }
    fn record_debug(&mut self, _: &Field, _: &dyn fmt::Debug) {}
}
impl<S: Subscriber + for<'span> LookupSpan<'span>> Layer<S> for WorkflowSignals {
    fn on_enter(&self, id: &Id, context: Context<'_, S>) {
        if context.span(id).unwrap().metadata().name() == "admit_audiobook_playback" {
            self.admitting.notify_one();
        }
    }
    fn on_event(&self, event: &Event<'_>, _: Context<'_, S>) {
        let mut preparation = Preparation::default();
        event.record(&mut preparation);
        if preparation.0 {
            self.prepared.notify_one();
        }
    }
}

pub async fn trace_position(
    mut position: tokio::sync::watch::Receiver<hito_playback::PlayerPosition>,
    started: tokio::sync::oneshot::Sender<()>,
    mut stop: tokio::sync::oneshot::Receiver<()>,
) -> usize {
    let initial = *position.borrow_and_update();
    let mut latest = initial.position_ms;
    let mut updates = 0;
    started.send(()).unwrap();
    loop {
        tokio::select! {
            biased;
            _ = &mut stop => return updates,
            changed = position.changed() => {
                changed.unwrap();
                let observed = *position.borrow_and_update();
                assert_eq!(observed.entry, initial.entry);
                assert_eq!(observed.selection, initial.selection);
                assert_eq!(observed.duration_ms, initial.duration_ms);
                assert!(observed.position_ms >= latest, "An exact receipt replay must not restart playback");
                latest = observed.position_ms;
                updates += 1;
                assert!(updates < 256, "The output observation exceeded its work budget");
            }
        }
    }
}
