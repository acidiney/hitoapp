use crate::{Error, Mp3Format};

pub(crate) const PREFIX_LIMIT: usize = 65_536;
const MAX_TAG_BYTES: usize = 61_440;

pub(crate) fn qualify(prefix: &[u8], length: u64) -> Result<Option<Mp3Format>, Error> {
    if !available(prefix, 4, length)? {
        return Ok(None);
    }
    let mut offset = 0;
    if prefix.starts_with(b"ID3") {
        if !available(prefix, 10, length)? {
            return Ok(None);
        }
        let version = prefix[3];
        let flags = prefix[5];
        let reserved_flags = if version == 3 { 0x1f } else { 0x0f };
        if !matches!(version, 3 | 4)
            || prefix[4] != 0
            || flags & reserved_flags != 0
            || prefix[6..10].iter().any(|byte| byte & 0x80 != 0)
        {
            return Err(Error::UnsupportedAudio);
        }
        let tag_bytes = prefix[6..10]
            .iter()
            .fold(0_usize, |size, byte| (size << 7) | usize::from(*byte));
        if tag_bytes > MAX_TAG_BYTES {
            return Err(Error::UnsupportedAudio);
        }
        let footer = version == 4 && flags & 0x10 != 0;
        offset = 10 + tag_bytes + usize::from(footer) * 10;
        if !available(prefix, offset + 4, length)? {
            return Ok(None);
        }
        if footer {
            let footer = &prefix[offset - 10..offset];
            if &footer[..3] != b"3DI" || footer[3..] != prefix[3..10] {
                return Err(Error::UnsupportedAudio);
            }
        }
    }
    let (first_bytes, format) = frame(&prefix[offset..])?;
    let next = offset + first_bytes;
    if !available(prefix, next + 4, length)? {
        return Ok(None);
    }
    let (second_bytes, second_format) = frame(&prefix[next..])?;
    if format != second_format {
        return Err(Error::UnsupportedAudio);
    }
    if !available(prefix, next + second_bytes, length)? {
        return Ok(None);
    }
    Ok(Some(format))
}

fn available(prefix: &[u8], needed: usize, length: u64) -> Result<bool, Error> {
    if needed > PREFIX_LIMIT || needed as u64 > length {
        return Err(Error::UnsupportedAudio);
    }
    Ok(prefix.len() >= needed)
}

fn frame(bytes: &[u8]) -> Result<(usize, Mp3Format), Error> {
    let header = u32::from_be_bytes(
        bytes
            .get(..4)
            .ok_or(Error::UnsupportedAudio)?
            .try_into()
            .map_err(|_| Error::UnsupportedAudio)?,
    );
    if header & 0xffe0_0000 != 0xffe0_0000
        || (header >> 19) & 3 != 3
        || (header >> 17) & 3 != 1
        || header & 3 == 2
    {
        return Err(Error::UnsupportedAudio);
    }
    let bitrate_index = ((header >> 12) & 15) as usize;
    let bitrates = [
        0_u32, 32, 40, 48, 56, 64, 80, 96, 112, 128, 160, 192, 224, 256, 320, 0,
    ];
    let bitrate = bitrates[bitrate_index] * 1000;
    let sample_rate = match (header >> 10) & 3 {
        0 => 44_100,
        1 => 48_000,
        2 => 32_000,
        _ => return Err(Error::UnsupportedAudio),
    };
    if bitrate == 0 {
        return Err(Error::UnsupportedAudio);
    }
    let frame_bytes = (144 * bitrate / sample_rate + ((header >> 9) & 1)) as usize;
    let channels = if (header >> 6) & 3 == 3 { 1 } else { 2 };
    Ok((
        frame_bytes,
        Mp3Format {
            sample_rate,
            channels,
        },
    ))
}
