pub mod capture;

use std::{
    fs::File,
    io::{Seek, SeekFrom, Write},
};

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
