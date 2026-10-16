use crate::{Error, Result};
use std::{fs::File, sync::LazyLock, time::Duration};
use symphonia::core::{
    codecs::audio::AudioDecoder,
    common::Limit,
    formats::{FormatReader, SeekMode, SeekTo, TrackType, probe::Hint},
    io::MediaSourceStream,
    meta::MetadataOptions,
    units::{Time, TimeBase},
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
    samples: Vec<f32>,
    position: Duration,
    pending: bool,
    seek_target: Option<Duration>,
    at_end: bool,
}

impl Decoder {
    #[tracing::instrument(level = "debug", skip_all, err(Debug))]
    pub fn open(file: File) -> Result<Self> {
        let reader = vendor(|| {
            symphonia::default::get_probe().probe(
                &Hint::new(),
                MediaSourceStream::new(Box::new(file), Default::default()),
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
            samples: Vec::new(),
            position: Duration::ZERO,
            pending: false,
            seek_target: None,
            at_end: false,
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
        self.at_end = self
            .format
            .duration
            .is_some_and(|duration| position >= duration);
        if !self.at_end {
            let time = Time::try_from_secs_f64(position.as_secs_f64()).ok_or(Error::Seek)?;
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
            if !(8_000..=384_000).contains(&rate)
                || !(1..=32).contains(&channels)
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
            self.format.sample_rate = rate;
            self.format.channels = channels as u16;
            self.position = self
                .time_base
                .calc_time(packet.pts)
                .and_then(|time| Duration::try_from_secs_f64(time.as_secs_f64()).ok())
                .unwrap_or(Duration::ZERO);
            self.samples.resize(audio.samples_interleaved(), 0.);
            audio.copy_to_slice_interleaved(&mut self.samples);
            if self.samples.iter().any(|sample| !sample.is_finite()) {
                return Err(Error::Decode);
            }
            return Ok(true);
        }
    }
}
