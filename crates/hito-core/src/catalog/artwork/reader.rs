use super::{Artwork, budget::BudgetedFile};
use crate::library_storage::{OpenedMedia, Root};
use lofty::{config::ParseOptions, file::TaggedFileExt, picture::PictureType, probe::Probe};
use std::{
    io::{BufReader, Read},
    path::Path,
};

const MAX_IMAGE_BYTES: usize = 8 * 1024 * 1024;
const MAX_METADATA_BYTES: usize = 16 * 1024 * 1024;

pub(super) fn read_artwork(audio: OpenedMedia) -> Option<Artwork> {
    read_embedded(audio.file).or_else(|| read_folder(&audio.root, audio.path.parent()?))
}

fn read_embedded(file: std::fs::File) -> Option<Artwork> {
    let mut reader = BudgetedFile::new(file, MAX_METADATA_BYTES);
    let tagged = Probe::new(BufReader::new(&mut reader))
        .options(ParseOptions::new().read_properties(false))
        .guess_file_type()
        .ok()?
        .read();
    if reader.exceeded() {
        tracing::debug!(
            event = "artwork_metadata_budget_exceeded",
            "Embedded artwork skipped"
        );
        return None;
    }
    let tagged = tagged.ok()?;
    for front_cover in [true, false] {
        for picture in tagged
            .tags()
            .iter()
            .flat_map(|tag| tag.pictures())
            .filter(|picture| (picture.pic_type() == PictureType::CoverFront) == front_cover)
        {
            if let Some(content_type) = raster_type(picture.data()) {
                return Some(Artwork {
                    content_type,
                    bytes: picture.data().to_vec(),
                });
            }
        }
    }
    None
}

fn read_folder(root: &Root, folder: &Path) -> Option<Artwork> {
    for name in [
        "cover.jpg",
        "cover.png",
        "folder.jpg",
        "folder.png",
        "front.jpg",
        "front.png",
    ] {
        let Ok(file) = root.open_file(&folder.join(name)) else {
            continue;
        };
        let mut bytes = Vec::new();
        if file
            .take(MAX_IMAGE_BYTES as u64 + 1)
            .read_to_end(&mut bytes)
            .is_err()
        {
            continue;
        }
        if let Some(content_type) = raster_type(&bytes) {
            return Some(Artwork {
                content_type,
                bytes,
            });
        }
    }
    None
}

fn raster_type(bytes: &[u8]) -> Option<&'static str> {
    if bytes.len() > MAX_IMAGE_BYTES {
        return None;
    }
    if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        Some("image/png")
    } else if bytes.starts_with(&[0xff, 0xd8, 0xff]) {
        Some("image/jpeg")
    } else if bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a") {
        Some("image/gif")
    } else if bytes.starts_with(b"RIFF") && bytes.get(8..12) == Some(b"WEBP") {
        Some("image/webp")
    } else {
        None
    }
}
