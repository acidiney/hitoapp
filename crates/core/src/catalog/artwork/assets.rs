use super::{Artwork, Catalog, target::Target};
use crate::{
    Actor, Error, Result,
    identity::recording_access,
    plugins::{MAX_ARTWORK_BYTES, ProviderArtwork},
};
use image::ImageFormat;
use sha2::{Digest, Sha256};
use sqlx::{QueryBuilder, Sqlite, SqliteConnection};
use std::io::Cursor;
use zune_core::{colorspace::ColorSpace, options::DecoderOptions};
use zune_jpeg::JpegDecoder;

pub(super) struct ValidatedArtwork {
    content_hash: String,
    content_type: &'static str,
    width: u32,
    height: u32,
    bytes: Vec<u8>,
}

pub(super) fn decode(remote: ProviderArtwork) -> Result<ValidatedArtwork> {
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
    let (width, height) = match format {
        ImageFormat::Png => decode_png(&remote.bytes)?,
        ImageFormat::Jpeg => decode_jpeg(&remote.bytes)?,
        _ => return Err(invalid()),
    };
    if width == 0 || height == 0 || width > 1024 || height > 1024 {
        return Err(invalid());
    }
    Ok(ValidatedArtwork {
        content_hash: format!("{:x}", Sha256::digest(&remote.bytes)),
        content_type,
        width,
        height,
        bytes: remote.bytes,
    })
}

fn decode_png(bytes: &[u8]) -> Result<(u32, u32)> {
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
    reader.next_frame(&mut pixels).map_err(|_| invalid())?;
    reader.finish().map_err(|_| invalid())?;
    if reader.info().animation_control.is_some() || reader.info().frame_control.is_some() {
        return Err(invalid());
    }
    drop(reader);
    if cursor.position() != bytes.len() as u64 {
        return Err(invalid());
    }
    Ok((width, height))
}

fn decode_jpeg(bytes: &[u8]) -> Result<(u32, u32)> {
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
    drop(decoder);
    if cursor.position() != bytes.len() as u64 || !bytes.ends_with(&[0xff, 0xd9]) {
        return Err(invalid());
    }
    Ok((width as u32, height as u32))
}

impl Catalog {
    pub(super) async fn retained_artwork(
        &self,
        actor: &Actor,
        recording: &str,
    ) -> Result<Option<Artwork>> {
        let mut tx = self.store.pool.begin().await?;
        actor.check_session_on(&mut tx).await?;
        recording_access(&mut tx, actor, recording).await?;
        let row: Option<(String, Vec<u8>)> = sqlx::query_as(
            "SELECT asset.content_type,asset.bytes FROM recording_artwork retained
             JOIN artwork_assets asset ON asset.user_id = retained.user_id AND asset.content_hash = retained.content_hash
             JOIN recordings r ON r.id = retained.recording_id
             JOIN libraries l ON l.id = r.library_id AND l.owner_id = retained.user_id
             WHERE retained.user_id = ? AND retained.recording_id = ?",
        ).bind(actor.id()).bind(recording).fetch_optional(&mut *tx).await?;
        tx.commit().await?;
        actor.check_session(&self.store).await?;
        tracing::debug!(found = row.is_some(), "Retained recording artwork read");
        row.map(stored).transpose()
    }
}

pub(super) fn stored((content_type, bytes): (String, Vec<u8>)) -> Result<Artwork> {
    let content_type = match content_type.as_str() {
        "image/png" => "image/png",
        "image/jpeg" => "image/jpeg",
        _ => return Err(Error::Internal("Stored artwork type is invalid".into())),
    };
    Ok(Artwork {
        content_type,
        bytes,
    })
}

pub(super) async fn store(
    connection: &mut SqliteConnection,
    account: &str,
    image: ValidatedArtwork,
) -> Result<String> {
    sqlx::query("INSERT INTO artwork_assets(user_id,content_hash,content_type,width,height,bytes) VALUES (?,?,?,?,?,?) ON CONFLICT(user_id,content_hash) DO NOTHING")
        .bind(account).bind(&image.content_hash).bind(image.content_type).bind(i64::from(image.width))
        .bind(i64::from(image.height)).bind(&image.bytes).execute(&mut *connection).await?;
    Ok(image.content_hash)
}

pub(super) async fn publish(
    connection: &mut SqliteConnection,
    account: &str,
    target: &Target,
    fetch: &str,
    image: ValidatedArtwork,
) -> Result<()> {
    let hash = store(&mut *connection, account, image).await?;
    let retained = match target {
        Target::Recording { id, observation_id, .. } => sqlx::query(
            "INSERT INTO recording_artwork(user_id,recording_id,observation_id,fetch_id,content_hash) VALUES (?,?,?,?,?) ON CONFLICT(user_id,recording_id) DO UPDATE SET observation_id = excluded.observation_id,fetch_id = excluded.fetch_id,content_hash = excluded.content_hash",
        ).bind(account).bind(id).bind(observation_id),
        Target::Release { id, observation_id, .. } => sqlx::query(
            "INSERT INTO release_artwork(user_id,release_id,observation_id,fetch_id,content_hash) VALUES (?,?,?,?,?) ON CONFLICT(user_id,release_id) DO UPDATE SET observation_id = excluded.observation_id,fetch_id = excluded.fetch_id,content_hash = excluded.content_hash",
        ).bind(account).bind(id).bind(observation_id),
        Target::Playlist { id, .. } => sqlx::query(
            "INSERT INTO playlist_artwork(user_id,playlist_id,fetch_id,content_hash) VALUES (?,?,?,?) ON CONFLICT(user_id,playlist_id) DO UPDATE SET fetch_id = excluded.fetch_id,content_hash = excluded.content_hash",
        ).bind(account).bind(id),
    };
    retained
        .bind(fetch)
        .bind(&hash)
        .execute(&mut *connection)
        .await?;
    prune(connection, Some(account)).await?;
    Ok(())
}

pub(super) async fn prune(connection: &mut SqliteConnection, account: Option<&str>) -> Result<()> {
    let mut query: QueryBuilder<Sqlite> = QueryBuilder::new("DELETE FROM artwork_assets WHERE ");
    if let Some(account) = account {
        query
            .push("user_id = ")
            .push_bind(account.to_owned())
            .push(" AND ");
    }
    query.push(
        "NOT EXISTS(SELECT 1 FROM recording_artwork INDEXED BY recording_artwork_asset
            WHERE user_id = artwork_assets.user_id AND content_hash = artwork_assets.content_hash)
        AND NOT EXISTS(SELECT 1 FROM release_artwork INDEXED BY release_artwork_asset
            WHERE user_id = artwork_assets.user_id AND content_hash = artwork_assets.content_hash)
        AND NOT EXISTS(SELECT 1 FROM playlist_artwork INDEXED BY playlist_artwork_asset
            WHERE user_id = artwork_assets.user_id AND content_hash = artwork_assets.content_hash)
        AND NOT EXISTS(SELECT 1 FROM cover_art_lookups
            WHERE user_id = artwork_assets.user_id AND content_hash = artwork_assets.content_hash)",
    );
    query.build().execute(connection).await?;
    Ok(())
}

fn invalid() -> Error {
    Error::Invalid("The provider returned invalid artwork".into())
}
