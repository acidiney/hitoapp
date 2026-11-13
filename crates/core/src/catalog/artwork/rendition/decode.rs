use super::invalid;
use crate::Result;
use image::{DynamicImage, GrayAlphaImage, GrayImage, RgbImage, RgbaImage, metadata::Orientation};
use std::io::Cursor;
use zune_core::{colorspace::ColorSpace, options::DecoderOptions};
use zune_jpeg::JpegDecoder;

pub(super) fn png(bytes: &[u8]) -> Result<DynamicImage> {
    let mut cursor = Cursor::new(bytes);
    let mut options = png::DecodeOptions::default();
    options.set_ignore_checksums(false);
    options.set_skip_ancillary_crc_failures(false);
    options.set_ignore_text_chunk(true);
    options.set_ignore_iccp_chunk(true);
    let mut decoder = png::Decoder::new_with_options(&mut cursor, options);
    decoder.set_limits(png::Limits {
        bytes: 8 * 1024 * 1024,
    });
    decoder.set_transformations(png::Transformations::EXPAND | png::Transformations::STRIP_16);
    let header = decoder.read_header_info().map_err(|_| invalid())?;
    let (width, height) = (header.width, header.height);
    if width == 0 || height == 0 || width > 1024 || height > 1024 {
        return Err(invalid());
    }
    let mut reader = decoder.read_info().map_err(|_| invalid())?;
    if reader.info().animation_control.is_some() || reader.info().frame_control.is_some() {
        return Err(invalid());
    }
    let output_bytes = reader
        .output_buffer_size()
        .filter(|size| *size > 0 && *size <= 8 * 1024 * 1024)
        .ok_or_else(invalid)?;
    let mut pixels = vec![0; output_bytes];
    let frame = reader.next_frame(&mut pixels).map_err(|_| invalid())?;
    reader.finish().map_err(|_| invalid())?;
    if reader.info().animation_control.is_some() || reader.info().frame_control.is_some() {
        return Err(invalid());
    }
    drop(reader);
    if cursor.position() != bytes.len() as u64 || frame.bit_depth != png::BitDepth::Eight {
        return Err(invalid());
    }
    pixels.truncate(frame.buffer_size());
    let image = match frame.color_type {
        png::ColorType::Grayscale => {
            GrayImage::from_raw(width, height, pixels).map(DynamicImage::ImageLuma8)
        }
        png::ColorType::GrayscaleAlpha => {
            GrayAlphaImage::from_raw(width, height, pixels).map(DynamicImage::ImageLumaA8)
        }
        png::ColorType::Rgb => {
            RgbImage::from_raw(width, height, pixels).map(DynamicImage::ImageRgb8)
        }
        png::ColorType::Rgba => {
            RgbaImage::from_raw(width, height, pixels).map(DynamicImage::ImageRgba8)
        }
        png::ColorType::Indexed => None,
    };
    image.ok_or_else(invalid)
}

pub(super) fn jpeg(bytes: &[u8]) -> Result<(DynamicImage, Orientation)> {
    let mut cursor = Cursor::new(bytes);
    let options = DecoderOptions::default()
        .set_strict_mode(true)
        .set_max_width(1024)
        .set_max_height(1024)
        .jpeg_set_max_scans(64)
        .jpeg_set_out_colorspace(ColorSpace::RGB);
    let mut decoder = JpegDecoder::new_with_options(&mut cursor, options);
    decoder.decode_headers().map_err(|_| invalid())?;
    let (width, height) = decoder.dimensions().ok_or_else(invalid)?;
    let components = decoder.info().ok_or_else(invalid)?.components;
    if width == 0
        || height == 0
        || width > 1024
        || height > 1024
        || !matches!(components, 1 | 3 | 4)
    {
        return Err(invalid());
    }
    let output_bytes = decoder
        .output_buffer_size()
        .filter(|size| *size > 0 && *size <= 8 * 1024 * 1024)
        .ok_or_else(invalid)?;
    let mut pixels = vec![0; output_bytes];
    decoder.decode_into(&mut pixels).map_err(|_| invalid())?;
    let orientation = decoder
        .exif()
        .and_then(|bytes| Orientation::from_exif_chunk(bytes))
        .unwrap_or(Orientation::NoTransforms);
    drop(decoder);
    if cursor.position() != bytes.len() as u64 || !bytes.ends_with(&[0xff, 0xd9]) {
        return Err(invalid());
    }
    let width = u32::try_from(width).map_err(|_| invalid())?;
    let height = u32::try_from(height).map_err(|_| invalid())?;
    let image = RgbImage::from_raw(width, height, pixels).ok_or_else(invalid)?;
    Ok((DynamicImage::ImageRgb8(image), orientation))
}
