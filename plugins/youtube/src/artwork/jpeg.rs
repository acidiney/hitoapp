use crate::Error;

const MAX_DIMENSION: u16 = 1024;

/// Checks the container structure the host requires: SOI, a supported start-of-frame
/// header within the dimension bound, and a final EOI marker. The host decodes fully.
pub(super) fn validate(bytes: &[u8]) -> Result<(), Error> {
    if bytes.len() < 4 || !bytes.starts_with(&[0xff, 0xd8]) || !bytes.ends_with(&[0xff, 0xd9]) {
        return Err(Error::InvalidMedia);
    }
    let mut position = 2;
    let mut frame = false;
    loop {
        if bytes.get(position) != Some(&0xff) {
            return Err(Error::InvalidMedia);
        }
        while bytes.get(position + 1) == Some(&0xff) {
            position += 1;
        }
        let marker = *bytes.get(position + 1).ok_or(Error::InvalidMedia)?;
        position += 2;
        match marker {
            0x00 | 0x01 | 0xd0..=0xd8 | 0xd9 => return Err(Error::InvalidMedia),
            0xda => {
                return if frame {
                    Ok(())
                } else {
                    Err(Error::InvalidMedia)
                };
            }
            _ => {}
        }
        let length = bytes
            .get(position..position + 2)
            .map(|pair| usize::from(u16::from_be_bytes([pair[0], pair[1]])))
            .filter(|length| *length >= 2)
            .ok_or(Error::InvalidMedia)?;
        let segment = bytes
            .get(position + 2..position + length)
            .ok_or(Error::InvalidMedia)?;
        if matches!(marker, 0xc0..=0xc2) {
            if frame || segment.len() < 6 {
                return Err(Error::InvalidMedia);
            }
            let height = u16::from_be_bytes([segment[1], segment[2]]);
            let width = u16::from_be_bytes([segment[3], segment[4]]);
            if height == 0 || width == 0 || height > MAX_DIMENSION || width > MAX_DIMENSION {
                return Err(Error::InvalidMedia);
            }
            frame = true;
        } else if matches!(marker, 0xc3 | 0xc5..=0xc7 | 0xc9..=0xcb | 0xcd..=0xcf) {
            return Err(Error::InvalidMedia);
        }
        position += length;
    }
}
