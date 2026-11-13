use image::{
    DynamicImage, ImageBuffer, ImageDecoder, ImageEncoder, ImageFormat, Rgb, RgbImage, Rgba,
    codecs::jpeg::JpegEncoder, metadata::Orientation,
};
use std::io::Cursor;

#[derive(Clone, Copy)]
pub enum Case {
    OrientedJpeg,
    Transparent,
    Palette,
    SixteenBit,
}

impl Case {
    pub fn source(self) -> (Vec<u8>, &'static str) {
        let bytes = match self {
            Self::OrientedJpeg => return (oriented_jpeg(), "image/jpeg"),
            Self::Palette => include_bytes!("artwork-rendition-palette.png").to_vec(),
            Self::Transparent => {
                let image = ImageBuffer::from_fn(1024, 768, |x, y| Rgba(rgba(x, y)));
                png(DynamicImage::ImageRgba8(image))
            }
            Self::SixteenBit => {
                let image = ImageBuffer::<Rgba<u16>, Vec<u16>>::from_fn(1024, 768, |x, y| {
                    Rgba(rgba(x, y).map(|channel| u16::from(channel) * 257))
                });
                png(DynamicImage::ImageRgba16(image))
            }
        };
        let decoded = image::load_from_memory(&bytes).unwrap().to_rgba8();
        assert_eq!(decoded.dimensions(), (1024, 768));
        assert_eq!(decoded.get_pixel(512, 384).0, [25, 180, 70, 128]);
        assert_eq!(decoded.get_pixel(10, 10).0[3], 0);
        (bytes, "image/png")
    }
}

fn rgba(x: u32, y: u32) -> [u8; 4] {
    if (256..768).contains(&x) && (192..576).contains(&y) {
        [25, 180, 70, 128]
    } else {
        [250, 0, 250, 0]
    }
}

fn png(image: DynamicImage) -> Vec<u8> {
    let mut bytes = Cursor::new(Vec::new());
    image.write_to(&mut bytes, ImageFormat::Png).unwrap();
    bytes.into_inner()
}

fn oriented_jpeg() -> Vec<u8> {
    let image = RgbImage::from_fn(1024, 768, |x, y| {
        Rgb(match (x < 512, y < 384) {
            (true, true) => [220, 40, 30],
            (false, true) => [40, 200, 60],
            (true, false) => [30, 60, 220],
            (false, false) => [220, 190, 30],
        })
    });
    let mut bytes = Vec::new();
    let mut encoder = JpegEncoder::new_with_quality(&mut bytes, 90);
    encoder
        .set_exif_metadata(vec![
            0x49, 0x49, 42, 0, 8, 0, 0, 0, 1, 0, 0x12, 1, 3, 0, 1, 0, 0, 0, 6, 0, 0, 0, 0, 0, 0, 0,
        ])
        .unwrap();
    encoder.encode_image(&image).unwrap();
    let mut decoder = image::codecs::jpeg::JpegDecoder::new(Cursor::new(&bytes)).unwrap();
    assert_eq!(decoder.orientation().unwrap(), Orientation::Rotate90);
    assert_eq!(decoder.dimensions(), (1024, 768));
    drop(decoder);
    bytes
}
