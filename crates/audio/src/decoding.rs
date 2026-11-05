use crate::{
    Error, Input, Result, capability,
    mp4_gapless::{Mp4Timing, Trim, read_mp4_timing},
};
use std::{io::SeekFrom, sync::LazyLock, time::Duration};
use symphonia::core::{
    audio::Channels,
    codecs::audio::AudioDecoder,
    common::Limit,
    formats::{FormatReader, SeekMode, SeekTo, TrackType, probe::Hint},
    io::MediaSourceStream,
    meta::MetadataOptions,
    units::{Time, TimeBase, Timestamp},
};

static VENDOR_LOGS: LazyLock<tracing::Dispatch> = LazyLock::new(tracing::Dispatch::none);

fn vendor<T>(work: impl FnOnce() -> T) -> T {
    tracing::dispatcher::with_default(&VENDOR_LOGS, work)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AudioFormat {
    pub sample_rate: u32,
    pub channels: u16,
    pub duration: Option<Duration>,
}

pub struct Decoded<'a> {
    pub samples: &'a [f32],
    pub position: Duration,
}

pub struct Decoder {
    reader: Box<dyn FormatReader>,
    codec: Box<dyn AudioDecoder>,
    track: u32,
    time_base: TimeBase,
    format: AudioFormat,
    layout: Channels,
    samples: Vec<f32>,
    position: Duration,
    pending: bool,
    seek_target: Option<Duration>,
    at_end: bool,
    declared: Option<Box<Mp4Timing>>,
    trim: Option<Trim>,
}

const SEEK_PREROLL_FRAMES: u64 = 2_048;

impl Decoder {
    #[tracing::instrument(level = "debug", skip_all, err(Debug))]
    pub fn open(mut input: impl Input + 'static) -> Result<Self> {
        let declared = if input.is_seekable() {
            let timing = read_mp4_timing(&mut input).map(Box::new);
            input.seek(SeekFrom::Start(0)).map_err(|_| Error::Decode)?;
            timing
        } else {
            None
        };
        let reader = vendor(|| {
            symphonia::default::get_probe().probe(
                &Hint::new(),
                MediaSourceStream::new(Box::new(input), Default::default()),
                Default::default(),
                MetadataOptions::default()
                    .limit_tag_bytes(Limit::Maximum(0))
                    .limit_visual_bytes(Limit::Maximum(0)),
            )
        })?;
        let track = reader
            .default_track(TrackType::Audio)
            .ok_or(Error::Unsupported)?;
        let parameters = track
            .codec_params
            .as_ref()
            .and_then(|parameters| parameters.audio())
            .ok_or(Error::Unsupported)?;
        let codec = vendor(|| {
            symphonia::default::get_codecs().make_audio_decoder(parameters, &Default::default())
        })?;
        let time_base = track.time_base.ok_or(Error::Unsupported)?;
        let duration = track
            .duration
            .and_then(|duration| time_base.calc_duration(duration))
            .and_then(|time| Duration::try_from_secs_f64(time.as_secs_f64()).ok());
        let mut decoder = Self {
            codec,
            track: track.id,
            time_base,
            format: AudioFormat {
                sample_rate: 0,
                channels: 0,
                duration,
            },
            reader,
            layout: Channels::None,
            samples: Vec::new(),
            position: Duration::ZERO,
            pending: false,
            seek_target: None,
            at_end: false,
            declared,
            trim: None,
        };
        if !decoder.decode(&mut || false)? {
            return Err(Error::Decode);
        }
        decoder.pending = true;
        tracing::debug!(
            event = "audio_decoder_ready",
            sample_rate = decoder.format.sample_rate,
            channels = decoder.format.channels,
            "Audio decoder ready"
        );
        Ok(decoder)
    }

    pub fn format(&self) -> AudioFormat {
        self.format
    }

    pub(crate) fn layout(&self) -> &Channels {
        &self.layout
    }

    #[tracing::instrument(level = "trace", skip_all)]
    pub fn read(&mut self) -> Result<Option<Decoded<'_>>> {
        self.read_until(|| false)
    }

    pub fn read_until(
        &mut self,
        mut interrupted: impl FnMut() -> bool,
    ) -> Result<Option<Decoded<'_>>> {
        if interrupted() {
            return Err(Error::Interrupted);
        }
        if self.at_end {
            return Ok(None);
        }
        loop {
            if self.pending {
                self.pending = false;
            } else if !self.decode(&mut interrupted)? {
                return Ok(None);
            }
            let frames = self.samples.len() / usize::from(self.format.channels);
            let skip = self.seek_target.map_or(0, |target| {
                let distance = target.saturating_sub(self.position).as_nanos();
                ((distance * u128::from(self.format.sample_rate) + 500_000_000) / 1_000_000_000)
                    .min(frames as u128) as usize
            });
            if skip == frames {
                continue;
            }
            self.seek_target = None;
            self.position +=
                Duration::from_secs_f64(skip as f64 / f64::from(self.format.sample_rate));
            return Ok(Some(Decoded {
                samples: &self.samples[skip * usize::from(self.format.channels)..],
                position: self.position,
            }));
        }
    }

    #[tracing::instrument(level = "debug", skip_all, err(Debug))]
    pub fn seek(&mut self, position: Duration) -> Result<()> {
        let source_position = self.source_seek_position(position);
        self.at_end = self
            .format
            .duration
            .is_some_and(|duration| position >= duration);
        if !self.at_end {
            let time = Time::try_from_secs_f64(source_position.as_secs_f64()).ok_or(Error::Seek)?;
            vendor(|| {
                self.reader.seek(
                    SeekMode::Accurate,
                    SeekTo::Time {
                        time,
                        track_id: Some(self.track),
                    },
                )
            })
            .map_err(|_| Error::Seek)?;
        }
        vendor(|| self.codec.reset());
        self.pending = false;
        self.seek_target = Some(position);
        tracing::debug!(
            event = "audio_decoder_seeked",
            position_ms = position.as_millis() as u64,
            "Audio decoder seeked"
        );
        Ok(())
    }

    fn source_seek_position(&self, position: Duration) -> Duration {
        let Some(trim) = self.trim else {
            return position;
        };
        let rate = f64::from(self.format.sample_rate);
        let source = position.as_secs_f64() + trim.start as f64 / rate;
        Duration::try_from_secs_f64((source - SEEK_PREROLL_FRAMES as f64 / rate).max(0.))
            .unwrap_or(Duration::ZERO)
    }

    fn resolve_trim(&mut self, rate: u32) {
        let Some(trim) = self.declared.take().and_then(|timing| timing.trim(rate)) else {
            return;
        };
        let source_frames = self
            .format
            .duration
            .map(|duration| (duration.as_secs_f64() * f64::from(rate)).round() as u64);
        if source_frames.is_some_and(|frames| trim.start.saturating_add(trim.length) > frames + 1) {
            tracing::debug!(
                event = "audio_gapless_declaration_rejected",
                "Gapless declaration exceeds the stream"
            );
            return;
        }
        self.format.duration = Some(frames_to_duration(trim.length, rate));
        self.trim = Some(trim);
        tracing::debug!(
            event = "audio_gapless_trim_applied",
            start_frames = trim.start,
            length_frames = trim.length,
            "Gapless trim applied"
        );
    }

    fn source_frame(&self, pts: Timestamp, rate: u32) -> u64 {
        self.time_base.calc_time(pts).map_or(0, |time| {
            (time.as_secs_f64() * f64::from(rate)).round() as u64
        })
    }

    #[tracing::instrument(level = "trace", skip_all, err(Debug))]
    fn decode(&mut self, interrupted: &mut impl FnMut() -> bool) -> Result<bool> {
        loop {
            if interrupted() {
                return Err(Error::Interrupted);
            }
            let Some(packet) = vendor(|| self.reader.next_packet())? else {
                return Ok(false);
            };
            if packet.track_id != self.track {
                continue;
            }
            let audio = vendor(|| self.codec.decode(&packet))?;
            let rate = audio.spec().rate();
            let channels = audio.spec().channels().count();
            if !u16::try_from(channels).is_ok_and(|channels| capability::supported(rate, channels))
                || audio.samples_interleaved() > 1_048_576
            {
                return Err(Error::Unsupported);
            }
            if self.format.sample_rate != 0
                && (rate != self.format.sample_rate
                    || channels != usize::from(self.format.channels))
            {
                return Err(Error::FormatChanged);
            }
            let first_packet = self.format.sample_rate == 0;
            if first_packet {
                self.layout = audio.spec().channels().clone();
            }
            self.format.sample_rate = rate;
            self.format.channels = channels as u16;
            self.position = self
                .time_base
                .calc_time(packet.pts)
                .and_then(|time| Duration::try_from_secs_f64(time.as_secs_f64()).ok())
                .unwrap_or(Duration::ZERO);
            self.samples.resize(audio.samples_interleaved(), 0.);
            audio.copy_to_slice_interleaved(&mut self.samples);
            if first_packet {
                self.resolve_trim(rate);
            }
            if let Some(trim) = self.trim {
                let first = self.source_frame(packet.pts, rate);
                let frames = (self.samples.len() / channels) as u64;
                let kept_start = first.max(trim.start);
                let kept_end = (first + frames).min(trim.start + trim.length);
                if kept_start >= kept_end {
                    if first >= trim.start + trim.length {
                        self.at_end = true;
                        return Ok(false);
                    }
                    continue;
                }
                self.samples
                    .truncate((kept_end - first) as usize * channels);
                self.samples
                    .drain(..(kept_start - first) as usize * channels);
                self.position = frames_to_duration(kept_start - trim.start, rate);
            }
            if self.samples.iter().any(|sample| !sample.is_finite()) {
                return Err(Error::Decode);
            }
            return Ok(true);
        }
    }
}

fn frames_to_duration(frames: u64, rate: u32) -> Duration {
    Duration::from_secs_f64(frames as f64 / f64::from(rate))
}
