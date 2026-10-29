use std::f64::consts::PI;

const TAPS_PER_FACTOR: usize = 32;
const KAISER_BETA: f64 = 9.;

/// Streaming linear-phase low-pass decimator for interleaved frames.
///
/// Output frame `k` is centred on input frame `factor * k`, so media time maps
/// directly to the decimated stream without a latency offset. Frames before the
/// start are treated as silence; `finish` supplies the trailing silence.
pub(crate) struct Decimator {
    factor: usize,
    channels: usize,
    taps: Vec<f32>,
    half: usize,
    window: Vec<f32>,
    start: i64,
    next: u64,
}

impl Decimator {
    pub fn new(factor: usize, channels: usize) -> Self {
        let half = TAPS_PER_FACTOR * factor;
        let mut decimator = Self {
            factor,
            channels,
            taps: design(factor, half),
            half,
            window: Vec::new(),
            start: 0,
            next: 0,
        };
        decimator.reset();
        decimator
    }

    pub fn reset(&mut self) {
        self.window.clear();
        self.window.resize(self.half * self.channels, 0.);
        self.start = -(self.half as i64);
        self.next = 0;
    }

    pub fn push(&mut self, input: &[f32], output: &mut Vec<f32>) {
        self.window.extend_from_slice(input);
        let width = 2 * self.half + 1;
        loop {
            let first = (self.factor as u64 * self.next) as i64 - self.half as i64 - self.start;
            let frames = (self.window.len() / self.channels) as i64;
            if first + width as i64 > frames {
                break;
            }
            let base = first as usize * self.channels;
            let frame = &self.window[base..base + width * self.channels];
            for channel in 0..self.channels {
                let mut sum = f64::from(self.taps[self.half])
                    * f64::from(frame[self.half * self.channels + channel]);
                for (tap, weight) in self.taps[..self.half].iter().enumerate() {
                    let outer = (width - 1 - tap) * self.channels + channel;
                    sum += f64::from(*weight)
                        * (f64::from(frame[tap * self.channels + channel])
                            + f64::from(frame[outer]));
                }
                output.push(sum as f32);
            }
            self.next += 1;
        }
        let keep = (self.factor as u64 * self.next) as i64 - self.half as i64;
        let drop = (keep - self.start).max(0) as usize;
        self.window.drain(..drop * self.channels);
        self.start += drop as i64;
    }

    pub fn finish(&mut self, output: &mut Vec<f32>) {
        let silence = vec![0.; self.half * self.channels];
        self.push(&silence, output);
    }
}

fn design(factor: usize, half: usize) -> Vec<f32> {
    let cutoff = 0.45 / factor as f64;
    let denominator = bessel(KAISER_BETA);
    let mut taps: Vec<f64> = (0..=2 * half)
        .map(|index| {
            let offset = index as f64 - half as f64;
            let ratio = offset / half as f64;
            let window = bessel(KAISER_BETA * (1. - ratio * ratio).max(0.).sqrt()) / denominator;
            let sinc = if offset == 0. {
                2. * cutoff
            } else {
                (2. * PI * cutoff * offset).sin() / (PI * offset)
            };
            sinc * window
        })
        .collect();
    let gain: f64 = taps.iter().sum();
    taps.iter_mut().for_each(|tap| *tap /= gain);
    taps.into_iter().map(|tap| tap as f32).collect()
}

fn bessel(x: f64) -> f64 {
    let (mut sum, mut term) = (1., 1.);
    for index in 1..64 {
        term *= (x / (2. * f64::from(index))).powi(2);
        sum += term;
        if term < sum * 1e-16 {
            break;
        }
    }
    sum
}
