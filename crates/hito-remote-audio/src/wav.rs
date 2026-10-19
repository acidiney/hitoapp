use crate::Error;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Pcm16WavFormat {
    pub sample_rate: u32,
    pub channels: u16,
    pub frames: u64,
}

pub(super) fn qualify(prefix: &[u8; 44], length: u64) -> Result<Pcm16WavFormat, Error> {
    let u16_at = |index| u16::from_le_bytes([prefix[index], prefix[index + 1]]);
    let u32_at = |index| {
        u32::from_le_bytes([
            prefix[index],
            prefix[index + 1],
            prefix[index + 2],
            prefix[index + 3],
        ])
    };
    let channels = u16_at(22);
    let sample_rate = u32_at(24);
    let alignment = channels.checked_mul(2).ok_or(Error::UnsupportedAudio)?;
    let byte_rate = sample_rate
        .checked_mul(u32::from(alignment))
        .ok_or(Error::UnsupportedAudio)?;
    let data_bytes = u64::from(u32_at(40));
    if &prefix[..4] != b"RIFF"
        || &prefix[8..16] != b"WAVEfmt "
        || &prefix[36..40] != b"data"
        || u32_at(16) != 16
        || u16_at(20) != 1
        || !(1..=8).contains(&channels)
        || !(8_000..=384_000).contains(&sample_rate)
        || u16_at(34) != 16
        || u16_at(32) != alignment
        || u32_at(28) != byte_rate
        || u64::from(u32_at(4)).checked_add(8) != Some(length)
        || data_bytes.checked_add(44) != Some(length)
        || data_bytes == 0
        || !data_bytes.is_multiple_of(u64::from(alignment))
    {
        return Err(Error::UnsupportedAudio);
    }
    Ok(Pcm16WavFormat {
        sample_rate,
        channels,
        frames: data_bytes / u64::from(alignment),
    })
}
