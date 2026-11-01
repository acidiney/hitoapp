mod model;

use super::{Client, transport::MAX_IMAGE_BYTES};
use crate::error::Error;
use axum::body::Bytes;
use hito_plugin_sdk::{ArtworkEntity, ArtworkRequest};
use image::ImageFormat;
use sha2::{Digest, Sha256};
use std::{io::Cursor, time::Instant};
use tokio::sync::OwnedSemaphorePermit;
use zune_core::{colorspace::ColorSpace, options::DecoderOptions};
use zune_jpeg::JpegDecoder;

const MAX_DIMENSION: u32 = 1024;
const MAX_DECODED_BYTES: usize = 8 * 1024 * 1024;
const MAX_DECODER_ALLOC_BYTES: u64 = 16 * 1024 * 1024;

pub(crate) struct Artwork {
    pub(crate) bytes: Bytes,
    pub(crate) content_type: &'static str,
    pub(crate) etag: String,
}

struct EncodedArtwork {
    bytes: Vec<u8>,
    _permit: OwnedSemaphorePermit,
}

impl AsRef<[u8]> for EncodedArtwork {
    fn as_ref(&self) -> &[u8] {
        &self.bytes
    }
}

impl Client {
    pub(crate) async fn artwork(&self, request: ArtworkRequest) -> Result<Artwork, Error> {
        let started = Instant::now();
        request
            .validate()
            .map_err(|_| Error::InvalidArtworkRequest)?;
        if request.entity == ArtworkEntity::Featured
            || !super::model::valid_id(&request.provider_id)
        {
            return Err(Error::InvalidArtworkRequest);
        }
        let permit = self
            .artwork_slots
            .clone()
            .try_acquire_owned()
            .map_err(|_| Error::Busy)?;
        let bootstrap = self
            .transport
            .artwork_bootstrap(request.entity, &request.provider_id)
            .await
            .map_err(Error::for_artwork)?;
        let location = model::location(&bootstrap, &request).map_err(Error::for_artwork)?;
        drop(bootstrap);
        let bytes = self
            .transport
            .image(&location)
            .await
            .map_err(Error::for_artwork)?;
        let artwork = tokio::task::spawn_blocking(move || validate_image(bytes, permit))
            .await
            .map_err(|_| Error::Unavailable)??;
        tracing::debug!(
            event = "spotify_artwork_completed",
            entity = ?request.entity,
            image_bytes = artwork.bytes.len(),
            elapsed_ms = started.elapsed().as_millis() as u64,
            "Spotify artwork resolved"
        );
        Ok(artwork)
    }
}

fn validate_image(bytes: Vec<u8>, permit: OwnedSemaphorePermit) -> Result<Artwork, Error> {
    if bytes.is_empty() || bytes.len() > MAX_IMAGE_BYTES {
        return Err(Error::InvalidArtworkResponse);
    }
    let content_type =
        match image::guess_format(&bytes).map_err(|_| Error::InvalidArtworkResponse)? {
            ImageFormat::Png => {
                validate_png(&bytes)?;
                "image/png"
            }
            ImageFormat::Jpeg => {
                validate_jpeg(&bytes)?;
                "image/jpeg"
            }
            _ => return Err(Error::InvalidArtworkResponse),
        };
    let etag = format!("\"{:x}\"", Sha256::digest(&bytes));
    Ok(Artwork {
        bytes: Bytes::from_owner(EncodedArtwork {
            bytes,
            _permit: permit,
        }),
        content_type,
        etag,
    })
}

fn validate_png(bytes: &[u8]) -> Result<(), Error> {
    let mut cursor = Cursor::new(bytes);
    let mut options = png::DecodeOptions::default();
    options.set_ignore_checksums(false);
    options.set_skip_ancillary_crc_failures(false);
    options.set_ignore_text_chunk(true);
    options.set_ignore_iccp_chunk(true);
    let mut decoder = png::Decoder::new_with_options(&mut cursor, options);
    decoder.set_limits(png::Limits {
        bytes: MAX_DECODER_ALLOC_BYTES as usize,
    });
    let header = decoder
        .read_header_info()
        .map_err(|_| Error::InvalidArtworkResponse)?;
    if header.width == 0
        || header.height == 0
        || header.width > MAX_DIMENSION
        || header.height > MAX_DIMENSION
    {
        return Err(Error::InvalidArtworkResponse);
    }
    let mut reader = decoder
        .read_info()
        .map_err(|_| Error::InvalidArtworkResponse)?;
    if reader.info().animation_control.is_some() || reader.info().frame_control.is_some() {
        return Err(Error::InvalidArtworkResponse);
    }
    let output_bytes = reader
        .output_buffer_size()
        .filter(|size| *size > 0 && *size <= MAX_DECODED_BYTES)
        .ok_or(Error::InvalidArtworkResponse)?;
    let mut pixels = vec![0; output_bytes];
    reader
        .next_frame(&mut pixels)
        .map_err(|_| Error::InvalidArtworkResponse)?;
    reader.finish().map_err(|_| Error::InvalidArtworkResponse)?;
    if reader.info().animation_control.is_some() || reader.info().frame_control.is_some() {
        return Err(Error::InvalidArtworkResponse);
    }
    drop(reader);
    if cursor.position() != bytes.len() as u64 {
        return Err(Error::InvalidArtworkResponse);
    }
    Ok(())
}

fn validate_jpeg(bytes: &[u8]) -> Result<(), Error> {
    let mut cursor = Cursor::new(bytes);
    let options = DecoderOptions::default()
        .set_strict_mode(true)
        .set_max_width(MAX_DIMENSION as usize)
        .set_max_height(MAX_DIMENSION as usize)
        .jpeg_set_max_scans(64)
        .jpeg_set_out_colorspace(ColorSpace::RGB);
    let mut decoder = JpegDecoder::new_with_options(&mut cursor, options);
    decoder
        .decode_headers()
        .map_err(|_| Error::InvalidArtworkResponse)?;
    let (width, height) = decoder.dimensions().ok_or(Error::InvalidArtworkResponse)?;
    let components = decoder
        .info()
        .ok_or(Error::InvalidArtworkResponse)?
        .components;
    if width == 0
        || height == 0
        || width > MAX_DIMENSION as usize
        || height > MAX_DIMENSION as usize
        || !matches!(components, 1 | 3 | 4)
    {
        return Err(Error::InvalidArtworkResponse);
    }
    let output_bytes = decoder
        .output_buffer_size()
        .filter(|size| *size > 0 && *size <= MAX_DECODED_BYTES)
        .ok_or(Error::InvalidArtworkResponse)?;
    let mut pixels = vec![0; output_bytes];
    decoder
        .decode_into(&mut pixels)
        .map_err(|_| Error::InvalidArtworkResponse)?;
    drop(decoder);
    if cursor.position() != bytes.len() as u64 || !bytes.ends_with(&[0xff, 0xd9]) {
        return Err(Error::InvalidArtworkResponse);
    }
    Ok(())
}
