use crate::{
    Error, Result,
    plugins::{MAX_ARTWORK_BYTES, ProviderArtwork},
};
use image::{
    DynamicImage, ImageFormat, codecs::jpeg::JpegEncoder, imageops::FilterType,
    metadata::Orientation,
};
use sha2::{Digest, Sha256};
use std::{io::Cursor, time::Instant};

mod decode;

const MAX_EDGE: u32 = 600;
const JPEG_QUALITY: u8 = 85;

pub(super) struct ValidatedArtwork {
    pub(super) content_hash: String,
    pub(super) content_type: &'static str,
    pub(super) width: u32,
    pub(super) height: u32,
    pub(super) bytes: Vec<u8>,
}

#[tracing::instrument(level = "debug", skip_all, err(Debug))]
pub(super) fn decode(remote: ProviderArtwork) -> Result<ValidatedArtwork> {
    let started = Instant::now();
    if remote.bytes.is_empty() || remote.bytes.len() > MAX_ARTWORK_BYTES {
        return Err(invalid());
    }
    let format = image::guess_format(&remote.bytes).map_err(|_| invalid())?;
    let content_type = match format {
        ImageFormat::Png => "image/png",
        ImageFormat::Jpeg => "image/jpeg",
        _ => return Err(invalid()),
    };
    if remote.content_type != content_type {
        return Err(invalid());
    }
    let (mut image, orientation) = match format {
        ImageFormat::Png => (decode::png(&remote.bytes)?, Orientation::NoTransforms),
        ImageFormat::Jpeg => decode::jpeg(&remote.bytes)?,
        _ => return Err(invalid()),
    };
    let (source_width, source_height) = (image.width(), image.height());
    let source_bytes = remote.bytes.len();
    let resized = source_width.max(source_height) > MAX_EDGE;
    let (bytes, content_type, width, height) = if resized {
        image.apply_orientation(orientation);
        let image = resized_image(image);
        let (width, height) = (image.width(), image.height());
        let (bytes, content_type) = encode(&image)?;
        if bytes.is_empty() || bytes.len() > MAX_ARTWORK_BYTES {
            return Err(invalid());
        }
        (bytes, content_type, width, height)
    } else {
        (remote.bytes, content_type, source_width, source_height)
    };
    tracing::debug!(
        outcome = if resized { "resized" } else { "preserved" },
        source_width,
        source_height,
        source_bytes,
        width,
        height,
        retained_bytes = bytes.len(),
        elapsed_us = started.elapsed().as_micros() as u64,
        "Managed artwork rendition validated"
    );
    Ok(ValidatedArtwork {
        content_hash: format!("{:x}", Sha256::digest(&bytes)),
        content_type,
        width,
        height,
        bytes,
    })
}

fn resized_image(image: DynamicImage) -> DynamicImage {
    let transparent = match &image {
        DynamicImage::ImageLumaA8(pixels) => pixels.pixels().any(|pixel| pixel.0[1] != 255),
        DynamicImage::ImageRgba8(pixels) => pixels.pixels().any(|pixel| pixel.0[3] != 255),
        _ => false,
    };
    if !transparent {
        return DynamicImage::ImageRgb8(image.into_rgb8()).resize(
            MAX_EDGE,
            MAX_EDGE,
            FilterType::Triangle,
        );
    }
    let mut pixels = image.into_rgba32f();
    for pixel in pixels.pixels_mut() {
        let alpha = pixel.0[3];
        for channel in &mut pixel.0[..3] {
            *channel *= alpha;
        }
    }
    let mut resized = DynamicImage::ImageRgba32F(pixels)
        .resize(MAX_EDGE, MAX_EDGE, FilterType::Triangle)
        .into_rgba32f();
    for pixel in resized.pixels_mut() {
        let alpha = pixel.0[3];
        for channel in &mut pixel.0[..3] {
            *channel = if alpha > 0.0 {
                (*channel / alpha).clamp(0.0, 1.0)
            } else {
                0.0
            };
        }
    }
    DynamicImage::ImageRgba8(DynamicImage::ImageRgba32F(resized).into_rgba8())
}

fn encode(image: &DynamicImage) -> Result<(Vec<u8>, &'static str)> {
    let mut output = Cursor::new(Vec::new());
    let content_type = if image.color().has_alpha() {
        image
            .write_to(&mut output, ImageFormat::Png)
            .map_err(|_| Error::Internal("Artwork rendition could not be encoded".into()))?;
        "image/png"
    } else {
        JpegEncoder::new_with_quality(&mut output, JPEG_QUALITY)
            .encode_image(image)
            .map_err(|_| Error::Internal("Artwork rendition could not be encoded".into()))?;
        "image/jpeg"
    };
    Ok((output.into_inner(), content_type))
}

fn invalid() -> Error {
    Error::Invalid("The provider returned invalid artwork".into())
}
