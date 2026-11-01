use std::f64::consts::PI;

const TAPS_PER_FACTOR: usize = 32;
const KAISER_BETA: f64 = 9.;

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
    let mut taps: Vec<f64> = (0..=2 * half)
        .map(|index| kernel(index as f64 - half as f64, cutoff, half))
        .collect();
    let gain: f64 = taps.iter().sum();
    taps.iter_mut().for_each(|tap| *tap /= gain);
    taps.into_iter().map(|tap| tap as f32).collect()
}

fn kernel(offset: f64, cutoff: f64, half: usize) -> f64 {
    let ratio = offset / half as f64;
    if ratio.abs() > 1. {
        return 0.;
    }
    let window = bessel(KAISER_BETA * (1. - ratio * ratio).max(0.).sqrt()) / bessel(KAISER_BETA);
    let sinc = if offset == 0. {
        2. * cutoff
    } else {
        (2. * PI * cutoff * offset).sin() / (PI * offset)
    };
    sinc * window
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

const MAX_PHASES: u64 = 4_096;

pub(crate) struct Resampler {
    channels: usize,
    half: usize,
    step: u64,
    period: u64,
    phases: usize,
    taps: Vec<f32>,
    window: Vec<f32>,
    start: i64,
    next: u64,
}

impl Resampler {
    pub fn new(input_rate: u32, output_rate: u32, channels: usize) -> Self {
        let divisor = gcd(u64::from(input_rate), u64::from(output_rate));
        let step = u64::from(input_rate) / divisor;
        let period = u64::from(output_rate) / divisor;
        let phases = period.min(MAX_PHASES) as usize;
        let reduction = (f64::from(input_rate) / f64::from(output_rate)).max(1.);
        let half = (TAPS_PER_FACTOR as f64 * reduction).ceil() as usize;
        let cutoff = 0.45 / reduction;
        let width = 2 * half;
        let mut taps = Vec::with_capacity(phases * width);
        for phase in 0..phases {
            let fraction = phase as f64 / phases as f64;
            let mut row: Vec<f64> = (0..width)
                .map(|tap| kernel(fraction + half as f64 - 1. - tap as f64, cutoff, half))
                .collect();
            let gain: f64 = row.iter().sum();
            row.iter_mut().for_each(|weight| *weight /= gain);
            taps.extend(row.into_iter().map(|weight| weight as f32));
        }
        let mut resampler = Self {
            channels,
            half,
            step,
            period,
            phases,
            taps,
            window: Vec::new(),
            start: 0,
            next: 0,
        };
        resampler.reset();
        resampler
    }

    pub fn reset(&mut self) {
        let lead = self.half - 1;
        self.window.clear();
        self.window.resize(lead * self.channels, 0.);
        self.start = -(lead as i64);
        self.next = 0;
    }

    pub fn push(&mut self, input: &[f32], output: &mut Vec<f32>) {
        self.window.extend_from_slice(input);
        let width = 2 * self.half;
        loop {
            let (base, phase) = self.position(self.next);
            let first = base - (self.half as i64 - 1) - self.start;
            let frames = (self.window.len() / self.channels) as i64;
            if first + width as i64 > frames {
                break;
            }
            let weights = &self.taps[phase * width..(phase + 1) * width];
            let frame = &self.window[first as usize * self.channels..];
            for channel in 0..self.channels {
                let sum: f64 = weights
                    .iter()
                    .enumerate()
                    .map(|(tap, weight)| {
                        f64::from(*weight) * f64::from(frame[tap * self.channels + channel])
                    })
                    .sum();
                output.push(sum as f32);
            }
            self.next += 1;
        }
        let (base, _) = self.position(self.next);
        let keep = base - (self.half as i64 - 1);
        let drop = (keep - self.start).max(0) as usize;
        self.window.drain(..drop * self.channels);
        self.start += drop as i64;
    }

    pub fn finish(&mut self, output: &mut Vec<f32>) {
        let silence = vec![0.; self.half * self.channels];
        self.push(&silence, output);
    }

    fn position(&self, index: u64) -> (i64, usize) {
        let scaled = u128::from(index) * u128::from(self.step);
        let base = (scaled / u128::from(self.period)) as i64;
        let remainder = (scaled % u128::from(self.period)) as u64;
        let phase = (remainder * self.phases as u64 + self.period / 2) / self.period;
        if phase as usize == self.phases {
            (base + 1, 0)
        } else {
            (base, phase as usize)
        }
    }
}

fn gcd(a: u64, b: u64) -> u64 {
    if b == 0 { a } else { gcd(b, a % b) }
}
