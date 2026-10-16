use crate::{catalog::Catalog, pcm};
use sha2::{Digest, Sha256};
use sqlx::{Row, SqliteConnection};
use std::{collections::BTreeMap, io};

pub(crate) async fn create(connection: &mut SqliteConnection) -> Result<(), sqlx::Error> {
    sqlx::query("CREATE TABLE evidence (recording_id TEXT NOT NULL REFERENCES recordings(id), kind TEXT NOT NULL, digest TEXT NOT NULL, PRIMARY KEY(recording_id, kind, digest))")
        .execute(&mut *connection).await?;
    sqlx::query("CREATE INDEX evidence_match ON evidence(kind, digest, recording_id)")
        .execute(&mut *connection)
        .await?;
    sqlx::query("CREATE TABLE representations (id TEXT PRIMARY KEY, recording_id TEXT NOT NULL REFERENCES recordings(id), checksum TEXT NOT NULL, active INTEGER NOT NULL CHECK(active IN (0, 1)))")
        .execute(&mut *connection).await?;
    sqlx::query(
        "CREATE INDEX active_representation ON representations(recording_id, checksum, active)",
    )
    .execute(&mut *connection)
    .await?;
    sqlx::query("INSERT INTO evidence SELECT id, 'file-v1', CASE WHEN substr(checksum, 1, 10) = 'recording-' THEN substr(checksum, 11) ELSE checksum END FROM recordings")
        .execute(&mut *connection).await?;
    sqlx::query("INSERT INTO representations SELECT 'source-' || id, id, CASE WHEN substr(checksum, 1, 10) = 'recording-' THEN substr(checksum, 11) ELSE checksum END, 1 FROM recordings")
        .execute(&mut *connection).await?;
    Ok(())
}

pub(crate) async fn reconcile(
    connection: &mut SqliteConnection,
    catalog: &mut Catalog,
    bootstrap: bool,
) -> io::Result<()> {
    let mut tracks = BTreeMap::new();
    let mut active = Vec::new();
    for (_, mut track) in std::mem::take(&mut catalog.tracks) {
        if !bootstrap {
            let exact = candidates(connection, "file-v1", &track.checksum).await?;
            let found = if exact.len() == 1 {
                exact.into_iter().next()
            } else if exact.is_empty() {
                if let Some(evidence) = &track.evidence {
                    let audio = candidates(connection, pcm::ALGORITHM, evidence).await?;
                    if audio.len() == 1 {
                        audio.into_iter().next()
                    } else {
                        None
                    }
                } else {
                    None
                }
            } else {
                None
            };
            track.id = found.unwrap_or_else(|| format!("recording-{}", uuid::Uuid::new_v4()));
            let previous: Option<String> = sqlx::query_scalar("SELECT id FROM representations WHERE recording_id = ? AND checksum = ? AND active = 1 LIMIT 1")
                .bind(&track.id).bind(&track.checksum)
                .fetch_optional(&mut *connection).await.map_err(state_error)?;
            track.source = previous.unwrap_or_else(|| format!("source-{}", uuid::Uuid::new_v4()));
        }
        let mut detail = track.recording();
        detail.revision = None;
        let revision = serde_json::to_vec(&(&detail, &track.source, &track.checksum))
            .map_err(io::Error::other)?;
        track.revision = format!("observation-{:x}", Sha256::digest(revision));
        let detail = serde_json::to_string(&track.recording()).map_err(io::Error::other)?;
        sqlx::query("INSERT INTO recordings (id, checksum, detail) VALUES (?, ?, ?) ON CONFLICT (id) DO UPDATE SET detail = excluded.detail")
            .bind(&track.id).bind(&track.checksum).bind(detail)
            .execute(&mut *connection).await.map_err(state_error)?;
        sqlx::query("INSERT OR IGNORE INTO evidence VALUES (?, 'file-v1', ?)")
            .bind(&track.id)
            .bind(&track.checksum)
            .execute(&mut *connection)
            .await
            .map_err(state_error)?;
        if let Some(evidence) = &track.evidence {
            sqlx::query("INSERT OR IGNORE INTO evidence VALUES (?, ?, ?)")
                .bind(&track.id)
                .bind(pcm::ALGORITHM)
                .bind(evidence)
                .execute(&mut *connection)
                .await
                .map_err(state_error)?;
        }
        active.push((
            track.source.clone(),
            track.id.clone(),
            track.checksum.clone(),
        ));
        tracks.entry(track.id.clone()).or_insert(track);
    }
    sqlx::query("UPDATE representations SET active = 0 WHERE active = 1")
        .execute(&mut *connection)
        .await
        .map_err(state_error)?;
    for (source, recording, checksum) in active {
        let inserted = sqlx::query("INSERT INTO representations VALUES (?, ?, ?, 1) ON CONFLICT (id) DO UPDATE SET active = 1 WHERE representations.recording_id = excluded.recording_id AND representations.checksum = excluded.checksum")
            .bind(source).bind(recording).bind(checksum)
            .execute(&mut *connection).await.map_err(state_error)?;
        if inserted.rows_affected() != 1 {
            return Err(io::Error::other("Folder representation binding changed"));
        }
    }
    for table in ["evidence", "representations"] {
        let query = format!("SELECT count(*) FROM {table}");
        let count: i64 = sqlx::query_scalar(&query)
            .fetch_one(&mut *connection)
            .await
            .map_err(state_error)?;
        if count > 100000 {
            return Err(io::Error::other("Folder identity history limit exceeded"));
        }
    }
    catalog.sources = tracks
        .values()
        .map(|track| (track.source_id(), track.id.clone()))
        .collect();
    catalog.tracks = tracks;
    Ok(())
}

async fn candidates(
    connection: &mut SqliteConnection,
    kind: &str,
    digest: &str,
) -> io::Result<Vec<String>> {
    let rows =
        sqlx::query("SELECT recording_id FROM evidence WHERE kind = ? AND digest = ? LIMIT 2")
            .bind(kind)
            .bind(digest)
            .fetch_all(&mut *connection)
            .await
            .map_err(state_error)?;
    Ok(rows.into_iter().map(|row| row.get(0)).collect())
}

fn state_error(_: sqlx::Error) -> io::Error {
    io::Error::other("Folder identity operation failed")
}
