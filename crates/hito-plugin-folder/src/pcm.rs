use crate::inspection::{InspectionError, WorkBudget, check};
use sha2::{Digest, Sha256};
use std::{
    fs::File,
    io::{Seek, SeekFrom},
    time::Instant,
};

pub(crate) const ALGORITHM: &str = "riff-pcm-s16-v1";

pub(crate) fn evidence(
    file: &mut File,
    size: u64,
    deadline: Instant,
    work: &mut WorkBudget,
) -> Result<Option<String>, InspectionError> {
    if size < 12 {
        return Ok(None);
    }
    let mut header = [0; 12];
    work.read_exact(file, &mut header, deadline)?;
    if &header[..4] != b"RIFF"
        || &header[8..] != b"WAVE"
        || u64::from(u32::from_le_bytes(header[4..8].try_into().unwrap())) + 8 != size
    {
        return Ok(None);
    }
    let mut position = 12u64;
    let mut format = None;
    let mut digest = None;
    let mut chunks = 0;
    while position < size {
        check(deadline)?;
        chunks += 1;
        if chunks > 4096 || size - position < 8 {
            return Ok(None);
        }
        let mut chunk = [0; 8];
        work.read_exact(file, &mut chunk, deadline)?;
        let length = u64::from(u32::from_le_bytes(chunk[4..].try_into().unwrap()));
        let end = position + 8 + length;
        let padded = end + length % 2;
        if padded > size {
            return Ok(None);
        }
        match &chunk[..4] {
            b"fmt " => {
                if format.is_some() || digest.is_some() || length != 16 {
                    return Ok(None);
                }
                let mut bytes = [0; 16];
                work.read_exact(file, &mut bytes, deadline)?;
                let codec = u16::from_le_bytes(bytes[..2].try_into().unwrap());
                let channels = u16::from_le_bytes(bytes[2..4].try_into().unwrap());
                let rate = u32::from_le_bytes(bytes[4..8].try_into().unwrap());
                let byte_rate = u32::from_le_bytes(bytes[8..12].try_into().unwrap());
                let alignment = u16::from_le_bytes(bytes[12..14].try_into().unwrap());
                let bits = u16::from_le_bytes(bytes[14..].try_into().unwrap());
                if codec != 1
                    || !(1..=2).contains(&channels)
                    || !(8000..=192000).contains(&rate)
                    || bits != 16
                    || alignment != channels * 2
                    || byte_rate != rate * u32::from(alignment)
                {
                    return Ok(None);
                }
                format = Some((bytes, alignment));
            }
            b"data" => {
                let Some((profile, alignment)) = format else {
                    return Ok(None);
                };
                if digest.is_some() || length == 0 || length % u64::from(alignment) != 0 {
                    return Ok(None);
                }
                let mut hash = Sha256::new();
                hash.update(ALGORITHM);
                hash.update(profile);
                hash.update(length.to_le_bytes());
                let mut remaining = length;
                let mut buffer = [0; 65536];
                while remaining > 0 {
                    check(deadline)?;
                    let wanted = remaining.min(buffer.len() as u64) as usize;
                    work.read_exact(file, &mut buffer[..wanted], deadline)?;
                    hash.update(&buffer[..wanted]);
                    remaining -= wanted as u64;
                }
                digest = Some(format!("{:x}", hash.finalize()));
            }
            _ => {}
        }
        file.seek(SeekFrom::Start(padded))
            .map_err(|_| InspectionError::Snapshot)?;
        position = padded;
    }
    Ok(digest)
}
