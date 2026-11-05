use super::{Actor, Identity, User};
use crate::{Error, Result};
use image::{
    DynamicImage, ImageDecoder, ImageFormat, ImageReader, Limits, Rgb, RgbImage,
    codecs::jpeg::JpegEncoder, imageops::FilterType,
};
use sha2::{Digest, Sha256};
use std::{io::Cursor, time::Instant};
use tokio::sync::Semaphore;

pub const MAX_AVATAR_UPLOAD_BYTES: usize = 2 * 1024 * 1024;
const MAX_SIDE: u32 = 8192;
const MAX_PIXELS: u64 = 16_000_000;
const MAX_ALLOCATION: u64 = 128 * 1024 * 1024;
const AVATAR_SIDE: u32 = 256;
const AVATAR_QUALITY: u8 = 88;
const CONTENT_TYPE: &str = "image/jpeg";

static PROCESSING: Semaphore = Semaphore::const_new(2);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AvatarImage {
    pub content_type: &'static str,
    pub version: String,
    pub bytes: Vec<u8>,
}

#[derive(Debug, Clone, Copy)]
enum Rejection {
    TooLarge,
    UnsupportedFormat,
    Dimensions,
    Corrupt,
}

impl Rejection {
    fn outcome(self) -> &'static str {
        match self {
            Self::TooLarge => "too_large",
            Self::UnsupportedFormat => "unsupported_format",
            Self::Dimensions => "dimensions",
            Self::Corrupt => "corrupt",
        }
    }

    fn message(self) -> &'static str {
        match self {
            Self::TooLarge => "Use a picture of 2 MiB or less",
            Self::UnsupportedFormat => "Use a JPEG, PNG or WebP picture",
            Self::Dimensions => "Use a picture of at most 16 megapixels",
            Self::Corrupt => "That picture could not be read",
        }
    }
}

fn size_bucket(length: usize) -> &'static str {
    match length {
        0..=65_535 => "under_64k",
        65_536..=524_287 => "under_512k",
        524_288..=MAX_AVATAR_UPLOAD_BYTES => "under_2m",
        _ => "over_2m",
    }
}

fn normalize(upload: &[u8]) -> std::result::Result<Vec<u8>, Rejection> {
    if upload.len() > MAX_AVATAR_UPLOAD_BYTES {
        return Err(Rejection::TooLarge);
    }
    let reader = ImageReader::new(Cursor::new(upload))
        .with_guessed_format()
        .map_err(|_| Rejection::Corrupt)?;
    if !matches!(
        reader.format(),
        Some(ImageFormat::Jpeg | ImageFormat::Png | ImageFormat::WebP)
    ) {
        return Err(Rejection::UnsupportedFormat);
    }
    let mut decoder = reader.into_decoder().map_err(|_| Rejection::Dimensions)?;
    let (width, height) = decoder.dimensions();
    if width == 0
        || height == 0
        || width > MAX_SIDE
        || height > MAX_SIDE
        || u64::from(width) * u64::from(height) > MAX_PIXELS
    {
        return Err(Rejection::Dimensions);
    }
    let mut limits = Limits::default();
    limits.max_alloc = Some(MAX_ALLOCATION);
    decoder
        .set_limits(limits)
        .map_err(|_| Rejection::Dimensions)?;
    let orientation = decoder
        .orientation()
        .unwrap_or(image::metadata::Orientation::NoTransforms);
    let mut picture = DynamicImage::from_decoder(decoder).map_err(|_| Rejection::Corrupt)?;
    picture.apply_orientation(orientation);
    let flattened = flatten(&picture);
    let square = DynamicImage::ImageRgb8(flattened)
        .resize_to_fill(AVATAR_SIDE, AVATAR_SIDE, FilterType::Lanczos3)
        .to_rgb8();
    let mut encoded = Vec::new();
    JpegEncoder::new_with_quality(&mut encoded, AVATAR_QUALITY)
        .encode_image(&square)
        .map_err(|_| Rejection::Corrupt)?;
    Ok(encoded)
}

fn flatten(picture: &DynamicImage) -> RgbImage {
    let rgba = picture.to_rgba8();
    RgbImage::from_fn(rgba.width(), rgba.height(), |x, y| {
        let pixel = rgba.get_pixel(x, y);
        let alpha = u32::from(pixel[3]);
        let blend = |channel: u8| ((u32::from(channel) * alpha + 255 * (255 - alpha)) / 255) as u8;
        Rgb([blend(pixel[0]), blend(pixel[1]), blend(pixel[2])])
    })
}

const USER_WITH_AVATAR: &str = "SELECT u.id, u.username, u.admin, u.display_name,
    (SELECT content_hash FROM account_avatars WHERE user_id = u.id) AS avatar_version
    FROM users u WHERE u.id = ?";

impl Identity {
    #[tracing::instrument(level = "info", skip_all, err(Debug), fields(account_id = actor.id()))]
    pub async fn set_avatar(&self, actor: &Actor, upload: Vec<u8>) -> Result<User> {
        actor.require_unlocked()?;
        actor.require_session()?;
        let started = Instant::now();
        let bucket = size_bucket(upload.len());
        let _permit = PROCESSING
            .acquire()
            .await
            .map_err(|_| Error::Internal("Avatar processing stopped".into()))?;
        let normalized = tokio::task::spawn_blocking(move || normalize(&upload))
            .await
            .map_err(|_| Error::Internal("Avatar processing failed".into()))?;
        let bytes = match normalized {
            Ok(bytes) => bytes,
            Err(rejection) => {
                tracing::info!(
                    event = "avatar_rejected",
                    outcome = rejection.outcome(),
                    size_bucket = bucket,
                    duration_ms = started.elapsed().as_millis() as u64,
                    "Avatar rejected"
                );
                return Err(Error::Invalid(rejection.message().into()));
            }
        };
        let hash = format!("{:x}", Sha256::digest(&bytes));
        let mut tx = self.store.pool.begin_with("BEGIN IMMEDIATE").await?;
        actor.check_session_on(&mut tx).await?;
        sqlx::query(
            "INSERT INTO account_avatars(user_id, content_hash, bytes) VALUES (?, ?, ?)
             ON CONFLICT(user_id) DO UPDATE SET content_hash = excluded.content_hash,
                 bytes = excluded.bytes, updated_at = unixepoch()",
        )
        .bind(actor.id())
        .bind(&hash)
        .bind(&bytes)
        .execute(&mut *tx)
        .await?;
        let user = sqlx::query_as(USER_WITH_AVATAR)
            .bind(actor.id())
            .fetch_optional(&mut *tx)
            .await?
            .ok_or(Error::Unauthorized)?;
        tx.commit().await?;
        tracing::info!(
            event = "avatar_changed",
            outcome = "stored",
            size_bucket = bucket,
            duration_ms = started.elapsed().as_millis() as u64,
            "Avatar changed"
        );
        Ok(user)
    }

    #[tracing::instrument(level = "info", skip_all, err(Debug), fields(account_id = actor.id()))]
    pub async fn remove_avatar(&self, actor: &Actor) -> Result<User> {
        actor.require_unlocked()?;
        actor.require_session()?;
        let started = Instant::now();
        let mut tx = self.store.pool.begin_with("BEGIN IMMEDIATE").await?;
        actor.check_session_on(&mut tx).await?;
        let removed = sqlx::query("DELETE FROM account_avatars WHERE user_id = ?")
            .bind(actor.id())
            .execute(&mut *tx)
            .await?
            .rows_affected();
        let user = sqlx::query_as(USER_WITH_AVATAR)
            .bind(actor.id())
            .fetch_optional(&mut *tx)
            .await?
            .ok_or(Error::Unauthorized)?;
        tx.commit().await?;
        tracing::info!(
            event = "avatar_removed",
            had_avatar = removed > 0,
            duration_ms = started.elapsed().as_millis() as u64,
            "Avatar removed"
        );
        Ok(user)
    }

    #[tracing::instrument(level = "debug", skip_all, err(Debug), fields(account_id = actor.id()))]
    pub async fn avatar(&self, actor: &Actor, account: &str) -> Result<AvatarImage> {
        actor.require_unlocked()?;
        if actor.id() != account && !actor.user().admin {
            return Err(Error::NotFound);
        }
        let mut connection = self.store.pool.acquire().await?;
        actor.check_session_on(&mut connection).await?;
        let row: Option<(String, Vec<u8>)> =
            sqlx::query_as("SELECT content_hash, bytes FROM account_avatars WHERE user_id = ?")
                .bind(account)
                .fetch_optional(&mut *connection)
                .await?;
        let (version, bytes) = row.ok_or(Error::NotFound)?;
        Ok(AvatarImage {
            content_type: CONTENT_TYPE,
            version,
            bytes,
        })
    }
}
