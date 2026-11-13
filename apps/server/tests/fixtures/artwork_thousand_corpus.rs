use image::{
    DynamicImage, ImageBuffer, ImageFormat, Rgb, RgbImage, Rgba, codecs::jpeg::JpegEncoder,
};
use sha2::{Digest, Sha256};
use std::{
    collections::HashSet,
    io::Cursor,
    path::{Path, PathBuf},
    sync::OnceLock,
};

pub const COUNT: usize = 1000;
pub const CLASSES: [&str; 5] = [
    "small_jpeg",
    "large_png",
    "large_jpeg",
    "transparent",
    "palette",
];

pub struct Corpus {
    directory: PathBuf,
    pub source_sizes: Vec<usize>,
    pub source_pixels: Vec<[u8; 32]>,
}

impl Corpus {
    pub async fn create(directory: &Path) -> Self {
        std::fs::create_dir(directory).unwrap();
        let mut source_sizes = Vec::new();
        let mut source_pixels = Vec::new();
        let mut encoded = HashSet::new();
        let mut pixels = HashSet::new();
        for index in 0..COUNT {
            let bytes = hito_core::test_support::hang_guard::hang_guard(
                "owned capacity source setup",
                async {
                    tokio::task::yield_now().await;
                    source(index)
                },
            )
            .await;
            assert!(
                !bytes.is_empty() && bytes.len() <= 2 * 1024 * 1024,
                "The frozen owned source must satisfy the SDK byte limit"
            );
            let decoded = image::load_from_memory(&bytes).unwrap();
            let hash: [u8; 32] = Sha256::digest(decoded.to_rgba8().as_raw()).into();
            assert!(
                encoded.insert(<[u8; 32]>::from(Sha256::digest(&bytes))),
                "Each owned source must have distinct encoded content"
            );
            assert!(
                pixels.insert(hash),
                "Each owned source must have distinct pixels"
            );
            source_pixels.push(hash);
            source_sizes.push(bytes.len());
            std::fs::write(directory.join(index.to_string()), bytes).unwrap();
        }
        Self {
            directory: directory.to_owned(),
            source_sizes,
            source_pixels,
        }
    }

    pub fn directory(&self) -> PathBuf {
        self.directory.clone()
    }

    pub fn verify(&self, index: usize, bytes: &[u8]) -> [u8; 32] {
        let actual = image::load_from_memory(bytes).unwrap().to_rgba8();
        let (width, height) = match index % 5 {
            0 => (500, 500),
            2 => (450, 600),
            _ => (600, 450),
        };
        assert!(
            actual.dimensions() == (width, height),
            "Managed dimensions and JPEG orientation must match the frozen size class"
        );
        if index % 5 == 4 {
            assert!(
                actual.get_pixel(10, 10).0[3] == 0,
                "Palette transparency must survive"
            );
            let center = actual.get_pixel(300, 225).0;
            assert!(
                center[3].abs_diff(128) <= 1,
                "Palette partial alpha must survive"
            );
            let original = image::load_from_memory(
                &std::fs::read(self.directory.join(index.to_string())).unwrap(),
            )
            .unwrap()
            .to_rgba8();
            let expected = original.get_pixel(512, 384).0;
            assert!(
                center[..3]
                    .iter()
                    .zip(expected)
                    .all(|(a, b)| a.abs_diff(b) <= 3),
                "Palette colors must survive"
            );
        } else {
            for bit in 0..10 {
                let sx = (bit * 90 + 75) as f64 / 1024.0;
                let sy = 60.0 / 768.0;
                let (x, y) = if index % 5 == 2 {
                    ((1.0 - sy) * f64::from(width), sx * f64::from(height))
                } else {
                    (sx * f64::from(width), sy * f64::from(height))
                };
                let expected = marker(index, bit);
                let observed = actual.get_pixel(x as u32, y as u32).0;
                assert!(
                    observed[..3]
                        .iter()
                        .zip(expected)
                        .all(|(a, b)| a.abs_diff(b) <= 25),
                    "Large identity cells must survive the managed codec and orientation"
                );
            }
            for (sx, sy) in [(100u32, 180u32), (400, 620), (900, 500)] {
                let (x, y) = if index % 5 == 2 {
                    (
                        (1.0 - f64::from(sy) / 768.0) * f64::from(width),
                        f64::from(sx) / 1024.0 * f64::from(height),
                    )
                } else {
                    (
                        f64::from(sx) / 1024.0 * f64::from(width),
                        f64::from(sy) / 768.0 * f64::from(height),
                    )
                };
                let observed = actual.get_pixel(x as u32, y as u32).0;
                assert!(
                    observed[..3]
                        .iter()
                        .zip(scene(index, sx, sy))
                        .all(|(a, b)| a.abs_diff(b) <= 40),
                    "Opaque scene color samples must survive resizing and the managed codec"
                );
            }
            if index % 5 == 3 {
                assert!(
                    actual.get_pixel(10, 440).0[3] == 0,
                    "Genuine transparent space must survive"
                );
                assert!(
                    actual.get_pixel(300, 225).0[3].abs_diff(128) <= 1,
                    "Partial alpha must survive"
                );
                let mut edges = 0;
                for x in 146..155 {
                    let edge = actual.get_pixel(x, 225).0;
                    if edge[3] > 0 && edge[3] < 128 {
                        edges += 1;
                        assert!(
                            edge[..3]
                                .iter()
                                .zip([25u8, 180, 70])
                                .all(|(a, b)| a.abs_diff(b) <= 3),
                            "Hidden colors must not bleed into transparent edges"
                        );
                    }
                }
                assert!(
                    edges > 0,
                    "The resized transparent edge must remain visible"
                );
            }
        }
        Sha256::digest(actual.as_raw()).into()
    }
}

pub fn content_type(index: usize) -> &'static str {
    if matches!(index % 5, 0 | 2) {
        "image/jpeg"
    } else {
        "image/png"
    }
}

fn source(index: usize) -> Vec<u8> {
    if index % 5 == 4 {
        return palette(index);
    }
    let (width, height) = if index.is_multiple_of(5) {
        (500, 500)
    } else {
        (1024, 768)
    };
    let image = RgbImage::from_fn(width, height, |x, y| {
        let sx = x * 1024 / width;
        let sy = y * 768 / height;
        if (24..96).contains(&sy) && (30..930).contains(&sx) {
            return Rgb(marker(index, ((sx - 30) / 90) as usize));
        }
        Rgb(scene(index, sx, sy))
    });
    if index % 5 == 3 {
        let rgba = ImageBuffer::from_fn(width, height, |x, y| {
            if (256..768).contains(&x) && (192..576).contains(&y) {
                Rgba([25, 180, 70, 128])
            } else if (128..256).contains(&x) && (192..576).contains(&y) || y > 600 && x < 200 {
                Rgba([250, 0, 250, 0])
            } else {
                let c = image.get_pixel(x, y).0;
                Rgba([c[0], c[1], c[2], 255])
            }
        });
        return png(DynamicImage::ImageRgba8(rgba));
    }
    if index % 5 == 1 {
        return png(DynamicImage::ImageRgb8(image));
    }
    let mut bytes = Vec::new();
    let mut encoder = JpegEncoder::new_with_quality(&mut bytes, 94);
    if index % 5 == 2 {
        use image::ImageEncoder;
        encoder
            .set_exif_metadata(vec![
                0x49, 0x49, 42, 0, 8, 0, 0, 0, 1, 0, 0x12, 1, 3, 0, 1, 0, 0, 0, 6, 0, 0, 0, 0, 0,
                0, 0,
            ])
            .unwrap();
    }
    encoder.encode_image(&image).unwrap();
    bytes
}

fn marker(index: usize, bit: usize) -> [u8; 3] {
    if index & (1 << bit) == 0 {
        [35, 60, 210]
    } else {
        [220, 185, 35]
    }
}

fn scene(index: usize, x: u32, y: u32) -> [u8; 3] {
    static SCENES: OnceLock<Vec<RgbImage>> = OnceLock::new();
    let scenes = SCENES.get_or_init(|| {
        (0..4)
            .map(|seed| RgbImage::from_fn(1024, 768, |x, y| Rgb(scene_base(seed, x, y))))
            .collect()
    });
    let ordinal = index / 5;
    let sample = scenes[ordinal % scenes.len()]
        .get_pixel((x + ordinal as u32 * 43) % 1024, y)
        .0;
    let tint = [
        (ordinal % 31) as i16 - 15,
        (ordinal % 23) as i16 - 11,
        (ordinal % 37) as i16 - 18,
    ];
    std::array::from_fn(|channel| (i16::from(sample[channel]) + tint[channel]).clamp(0, 255) as u8)
}

fn scene_base(seed: usize, x: u32, y: u32) -> [u8; 3] {
    let seed = seed as u32 + 1;
    let ridge = 330.0 + 65.0 * ((f64::from(x) / 88.0) + f64::from(seed) / 13.0).sin();
    let sky = f64::from(y) < ridge;
    let mut value = [0.0f64; 3];
    for (channel, out) in value.iter_mut().enumerate() {
        let base = if sky {
            [80.0, 125.0, 175.0][channel]
        } else {
            [95.0, 100.0, 65.0][channel]
        };
        *out = base + f64::from(y) / 22.0;
        for (scale, amplitude) in [(160, 45.0), (55, 28.0), (18, 19.0), (6, 12.0), (2, 7.0)] {
            *out += amplitude * smooth(seed + channel as u32 * 19, x, y, scale);
        }
        let leaf = ((x + seed * 7) / 19 + y / 13).is_multiple_of(11) && !sky;
        if leaf {
            *out += [25.0, 42.0, -10.0][channel];
        }
    }
    value.map(|v| v.clamp(0.0, 255.0).round() as u8)
}

fn smooth(seed: u32, x: u32, y: u32, scale: u32) -> f64 {
    let tx = f64::from(x % scale) / f64::from(scale);
    let ty = f64::from(y % scale) / f64::from(scale);
    let tx = tx * tx * (3.0 - 2.0 * tx);
    let ty = ty * ty * (3.0 - 2.0 * ty);
    let noise = |x: u32, y: u32| {
        let mut n =
            x.wrapping_mul(374761393) ^ y.wrapping_mul(668265263) ^ seed.wrapping_mul(2246822519);
        n = (n ^ (n >> 13)).wrapping_mul(1274126177);
        f64::from(n ^ (n >> 16)) / f64::from(u32::MAX) * 2.0 - 1.0
    };
    let a = noise(x / scale, y / scale) * (1.0 - tx) + noise(x / scale + 1, y / scale) * tx;
    let b = noise(x / scale, y / scale + 1) * (1.0 - tx) + noise(x / scale + 1, y / scale + 1) * tx;
    a * (1.0 - ty) + b * ty
}

fn png(image: DynamicImage) -> Vec<u8> {
    let mut bytes = Cursor::new(Vec::new());
    image.write_to(&mut bytes, ImageFormat::Png).unwrap();
    bytes.into_inner()
}

fn palette(index: usize) -> Vec<u8> {
    let mut bytes = include_bytes!("artwork-rendition-palette.png").to_vec();
    let mut offset = 8;
    let mut changed = false;
    while offset < bytes.len() {
        let length = u32::from_be_bytes(bytes[offset..offset + 4].try_into().unwrap()) as usize;
        let end = offset + 8 + length;
        if &bytes[offset + 4..offset + 8] == b"PLTE" {
            for (entry, color) in bytes[offset + 8..end]
                .as_chunks_mut::<3>()
                .0
                .iter_mut()
                .enumerate()
            {
                color[0] = ((index / 5 + entry * 31) % 200 + 25) as u8;
                color[1] = ((index / 5 * 7 + entry * 17) % 200 + 25) as u8;
                color[2] = ((index / 5 * 13 + entry * 11) % 200 + 25) as u8;
            }
            let crc = crc32(&bytes[offset + 4..end]);
            bytes[end..end + 4].copy_from_slice(&crc.to_be_bytes());
            changed = true;
        }
        offset = end + 4;
    }
    assert!(
        changed && bytes[25] == 3,
        "The owned source must remain an indexed PNG"
    );
    bytes
}

fn crc32(bytes: &[u8]) -> u32 {
    let mut crc = u32::MAX;
    for byte in bytes {
        crc ^= u32::from(*byte);
        for _ in 0..8 {
            crc = (crc >> 1) ^ (0xedb88320 & (0u32.wrapping_sub(crc & 1)));
        }
    }
    !crc
}

pub fn report(stage: &str, sizes: &[Option<usize>]) {
    for (class, name) in CLASSES.iter().enumerate() {
        let mut values: Vec<usize> = sizes
            .iter()
            .enumerate()
            .filter(|(i, _)| i % 5 == class)
            .filter_map(|(_, v)| *v)
            .collect();
        values.sort_unstable();
        if values.is_empty() {
            eprintln!("artwork_thousand stage={stage} class={name} count=0");
        } else {
            let at =
                |percent: usize| values[(values.len() * percent).div_ceil(100).saturating_sub(1)];
            eprintln!(
                "artwork_thousand stage={stage} class={name} count={} min={} p50={} p95={} max={} total={}",
                values.len(),
                values[0],
                at(50),
                at(95),
                values.last().unwrap(),
                values.iter().sum::<usize>()
            );
        }
    }
}
