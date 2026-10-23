const MAX_BYTES: usize = 2 * 1024 * 1024;
const MAX_DIMENSION: u32 = 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MediaArtwork {
    pub content_type: &'static str,
    pub bytes: Vec<u8>,
}

/// Accepts only PNG or JPEG whose header proves a size within the browser
/// bridge's bounds. The declared content type is not trusted.
pub(crate) fn bound(bytes: Vec<u8>) -> Option<MediaArtwork> {
    if bytes.is_empty() || bytes.len() > MAX_BYTES {
        return None;
    }
    let (content_type, width, height) = if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        let (width, height) = png_size(&bytes)?;
        ("image/png", width, height)
    } else if bytes.starts_with(&[0xFF, 0xD8]) {
        let (width, height) = jpeg_size(&bytes)?;
        ("image/jpeg", width, height)
    } else {
        return None;
    };
    ((1..=MAX_DIMENSION).contains(&width) && (1..=MAX_DIMENSION).contains(&height)).then_some(
        MediaArtwork {
            content_type,
            bytes,
        },
    )
}

fn be32(bytes: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_be_bytes(bytes.get(at..at + 4)?.try_into().ok()?))
}

fn be16(bytes: &[u8], at: usize) -> Option<u16> {
    Some(u16::from_be_bytes(bytes.get(at..at + 2)?.try_into().ok()?))
}

fn png_size(bytes: &[u8]) -> Option<(u32, u32)> {
    (bytes.get(12..16)? == b"IHDR").then_some(())?;
    Some((be32(bytes, 16)?, be32(bytes, 20)?))
}

fn jpeg_size(bytes: &[u8]) -> Option<(u32, u32)> {
    let mut at = 2;
    loop {
        if *bytes.get(at)? != 0xFF {
            return None;
        }
        let marker = *bytes.get(at + 1)?;
        match marker {
            0xFF => at += 1,
            0x01 | 0xD0..=0xD7 => at += 2,
            0xD8..=0xDA => return None,
            0xC0..=0xC3 | 0xC5..=0xC7 | 0xC9..=0xCB | 0xCD..=0xCF => {
                return Some((be16(bytes, at + 7)? as u32, be16(bytes, at + 5)? as u32));
            }
            _ => at += 2 + be16(bytes, at + 2)? as usize,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn png(width: u32, height: u32) -> Vec<u8> {
        let mut bytes = b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR".to_vec();
        bytes.extend(width.to_be_bytes());
        bytes.extend(height.to_be_bytes());
        bytes.extend([8, 6, 0, 0, 0]);
        bytes
    }
    fn jpeg(width: u16, height: u16) -> Vec<u8> {
        let mut bytes = vec![0xFF, 0xD8, 0xFF, 0xE0, 0, 4, 0, 0, 0xFF, 0xC0, 0, 11, 8];
        bytes.extend(height.to_be_bytes());
        bytes.extend(width.to_be_bytes());
        bytes.extend([1, 1, 0x11, 0]);
        bytes
    }

    #[test]
    fn png_and_jpeg_within_bounds_are_accepted() {
        assert_eq!(bound(png(1024, 1024)).unwrap().content_type, "image/png");
        assert_eq!(bound(jpeg(640, 480)).unwrap().content_type, "image/jpeg");
    }

    #[test]
    fn oversized_dimensions_are_rejected() {
        assert!(bound(png(1025, 100)).is_none());
        assert!(bound(png(100, 1025)).is_none());
        assert!(bound(jpeg(1025, 100)).is_none());
        assert!(bound(png(0, 10)).is_none());
    }

    #[test]
    fn oversized_bytes_are_rejected() {
        let mut bytes = png(10, 10);
        bytes.resize(2 * 1024 * 1024 + 1, 0);
        assert!(bound(bytes).is_none());
        let mut bytes = png(10, 10);
        bytes.resize(2 * 1024 * 1024, 0);
        assert!(bound(bytes).is_some());
    }

    #[test]
    fn other_formats_and_truncated_images_are_rejected() {
        assert!(bound(b"GIF89a\x01\0\x01\0".to_vec()).is_none());
        assert!(bound(png(10, 10)[..16].to_vec()).is_none());
        assert!(bound(vec![0xFF, 0xD8, 0xFF, 0xDA, 0, 2]).is_none());
        assert!(bound(Vec::new()).is_none());
    }
}
