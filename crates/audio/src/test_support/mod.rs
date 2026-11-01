pub mod capture;

use std::{
    fs::File,
    io::{Seek, SeekFrom, Write},
    sync::Mutex,
};

static OUTPUT_RATES: Mutex<Option<Vec<u32>>> = Mutex::new(None);

/// While alive, every player opened by this process sees a device that offers
/// only the listed sample rates.
pub struct OutputRates(());

pub fn restrict_output_rates(rates: &[u32]) -> OutputRates {
    *OUTPUT_RATES.lock().unwrap() = Some(rates.to_vec());
    OutputRates(())
}

impl Drop for OutputRates {
    fn drop(&mut self) {
        *OUTPUT_RATES.lock().unwrap() = None;
    }
}

#[cfg(feature = "device")]
pub(crate) fn restricted_output_rates() -> Option<Vec<u32>> {
    OUTPUT_RATES.lock().unwrap().clone()
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
