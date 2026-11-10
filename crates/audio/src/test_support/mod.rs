pub mod capture;
pub mod continuity;
pub mod m4a;
#[cfg(feature = "device")]
pub mod output_witness;

use std::{
    fs::File,
    io::{Seek, SeekFrom, Write},
    sync::{Mutex, MutexGuard, PoisonError},
};

static OUTPUT_RATES: Mutex<Option<Vec<u32>>> = Mutex::new(None);
static HOLDERS: Mutex<()> = Mutex::new(());

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

pub struct OutputRates {
    previous: Option<Vec<u32>>,
    _exclusive: MutexGuard<'static, ()>,
}

pub fn restrict_output_rates(rates: &[u32]) -> OutputRates {
    let exclusive = lock(&HOLDERS);
    let previous = lock(&OUTPUT_RATES).replace(rates.to_vec());
    OutputRates {
        previous,
        _exclusive: exclusive,
    }
}

impl Drop for OutputRates {
    fn drop(&mut self) {
        *lock(&OUTPUT_RATES) = self.previous.take();
    }
}

#[cfg(feature = "device")]
pub(crate) fn restricted_output_rates() -> Option<Vec<u32>> {
    lock(&OUTPUT_RATES).clone()
}

pub fn wave(rate: u32, channels: u16, frames: u32, sample: impl Fn(u32, u16) -> i16) -> File {
    let mut file = tempfile::tempfile().unwrap();
    let size = frames * u32::from(channels) * 2;
    file.write_all(b"RIFF").unwrap();
    file.write_all(&(size + 36).to_le_bytes()).unwrap();
    file.write_all(b"WAVEfmt ").unwrap();
    file.write_all(&16_u32.to_le_bytes()).unwrap();
    file.write_all(&1_u16.to_le_bytes()).unwrap();
    file.write_all(&channels.to_le_bytes()).unwrap();
    file.write_all(&rate.to_le_bytes()).unwrap();
    file.write_all(&(rate * u32::from(channels) * 2).to_le_bytes())
        .unwrap();
    file.write_all(&(channels * 2).to_le_bytes()).unwrap();
    file.write_all(&16_u16.to_le_bytes()).unwrap();
    file.write_all(b"data").unwrap();
    file.write_all(&size.to_le_bytes()).unwrap();
    for frame in 0..frames {
        for channel in 0..channels {
            file.write_all(&sample(frame, channel).to_le_bytes())
                .unwrap();
        }
    }
    file.seek(SeekFrom::Start(0)).unwrap();
    file
}
