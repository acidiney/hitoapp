use crate::{
    catalog::{Catalog, Track},
    pcm,
};
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
    catalog.tracks.clear();
    let mut observations = std::mem::take(&mut catalog.observations);
    observations.sort_by(|a, b| a.locator.cmp(&b.locator));
    let mut novel = BTreeMap::<String, String>::new();
    let mut sources = BTreeMap::<(String, String), String>::new();
    for track in &mut observations {
        if !bootstrap {
            let found = match continuity(connection, track).await? {
                Some(id) => Some(id),
                None => historical(connection, track).await?,
            };
            track.id = found.unwrap_or_else(|| {
                novel
                    .entry(track.checksum.clone())
                    .or_insert_with(|| format!("recording-{}", uuid::Uuid::new_v4()))
                    .clone()
            });
            let key = (track.id.clone(), track.checksum.clone());
            track.source = if let Some(source) = sources.get(&key) {
                source.clone()
            } else {
                let previous: Vec<String> = sqlx::query_scalar("SELECT id FROM representations WHERE recording_id = ? AND checksum = ? AND active = 1 LIMIT 2")
                        .bind(&track.id).bind(&track.checksum)
                        .fetch_all(&mut *connection).await.map_err(state_error)?;
                if previous.len() > 1 {
                    return Err(io::Error::other(
                        "Folder current representation is ambiguous",
                    ));
                }
                let source = previous
                    .into_iter()
                    .next()
                    .unwrap_or_else(|| format!("source-{}", uuid::Uuid::new_v4()));
                sources.insert(key, source.clone());
                source
            };
        }
    }
    crate::releases::reconcile(connection, &mut observations, bootstrap).await?;
    for track in &mut observations {
        let mut detail = track.recording();
        detail.revision = None;
        let revision = serde_json::to_vec(&(&detail, &track.source, &track.checksum))
            .map_err(io::Error::other)?;
        track.revision = format!("observation-{:x}", Sha256::digest(revision));
    }
    let mut selected = BTreeMap::<String, usize>::new();
    for (index, track) in observations.iter().enumerate() {
        selected.entry(track.id.clone()).or_insert(index);
    }
    for (recording, selected_index) in &mut selected {
        let previous: Option<Option<Vec<u8>>> =
            sqlx::query_scalar("SELECT owner_locator FROM recordings WHERE id = ?")
                .bind(recording)
                .fetch_optional(&mut *connection)
                .await
                .map_err(state_error)?;
        if let Some(Some(locator)) = previous
            && let Ok(index) = observations.binary_search_by(|track| track.locator.cmp(&locator))
            && observations[index].id == *recording
        {
            *selected_index = index;
        }
    }
    for index in selected.values() {
        let track = &observations[*index];
        let detail = serde_json::to_string(&track.recording()).map_err(io::Error::other)?;
        sqlx::query("INSERT INTO recordings (id, checksum, detail, owner_locator) VALUES (?, ?, ?, ?) ON CONFLICT (id) DO UPDATE SET detail = excluded.detail, owner_locator = excluded.owner_locator")
            .bind(&track.id).bind(&track.checksum).bind(detail).bind(&track.locator)
            .execute(&mut *connection).await.map_err(state_error)?;
    }
    crate::releases::state::observe(connection, &observations).await?;
    let mut active = BTreeMap::new();
    for track in &observations {
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
        active.insert(
            track.source.clone(),
            (track.id.clone(), track.checksum.clone()),
        );
    }
    sqlx::query("UPDATE representations SET active = 0 WHERE active = 1")
        .execute(&mut *connection)
        .await
        .map_err(state_error)?;
    for (source, (recording, checksum)) in active {
        let inserted = sqlx::query("INSERT INTO representations VALUES (?, ?, ?, 1) ON CONFLICT (id) DO UPDATE SET active = 1 WHERE representations.recording_id = excluded.recording_id AND representations.checksum = excluded.checksum")
            .bind(source).bind(recording).bind(checksum)
            .execute(&mut *connection).await.map_err(state_error)?;
        if inserted.rows_affected() != 1 {
            return Err(io::Error::other("Folder representation binding changed"));
        }
    }
    sqlx::query("DELETE FROM observations")
        .execute(&mut *connection)
        .await
        .map_err(state_error)?;
    for track in &observations {
        let detail = serde_json::to_string(&track.recording()).map_err(io::Error::other)?;
        sqlx::query("INSERT INTO observations (locator, recording_id, source_id, checksum, evidence_kind, evidence_digest, detail, release_id) VALUES (?, ?, ?, ?, ?, ?, ?, ?)")
            .bind(&track.locator).bind(&track.id).bind(&track.source).bind(&track.checksum)
            .bind(track.evidence.as_ref().map(|_| pcm::ALGORITHM)).bind(&track.evidence).bind(detail).bind(&track.release_id)
            .execute(&mut *connection).await.map_err(state_error)?;
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
    catalog.tracks = selected
        .into_iter()
        .map(|(id, index)| (id, observations[index].clone()))
        .collect();
    catalog.observations = observations;
    catalog.index_sources()
}

pub(crate) async fn create_owners(connection: &mut SqliteConnection) -> Result<(), sqlx::Error> {
    sqlx::query("ALTER TABLE recordings ADD COLUMN owner_locator BLOB")
        .execute(&mut *connection)
        .await?;
    sqlx::query(
        "CREATE INDEX observation_recording_locator ON observations(recording_id, locator)",
    )
    .execute(&mut *connection)
    .await?;
    sqlx::query("UPDATE recordings SET owner_locator = (SELECT locator FROM observations WHERE observations.recording_id = recordings.id AND observations.detail = recordings.detail ORDER BY locator LIMIT 1)")
        .execute(&mut *connection).await?;
    Ok(())
}

pub(crate) async fn create_observations(
    connection: &mut SqliteConnection,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "CREATE UNIQUE INDEX representation_recording ON representations(id, recording_id)",
    )
    .execute(&mut *connection)
    .await?;
    sqlx::query("CREATE TABLE observations (locator BLOB PRIMARY KEY NOT NULL CHECK(length(locator) BETWEEN 1 AND 16384), recording_id TEXT NOT NULL REFERENCES recordings(id), source_id TEXT NOT NULL, checksum TEXT NOT NULL, evidence_kind TEXT, evidence_digest TEXT, detail TEXT NOT NULL, CHECK((evidence_kind IS NULL) = (evidence_digest IS NULL)), FOREIGN KEY(source_id, recording_id) REFERENCES representations(id, recording_id))")
        .execute(&mut *connection).await?;
    Ok(())
}

async fn continuity(
    connection: &mut SqliteConnection,
    track: &Track,
) -> io::Result<Option<String>> {
    let previous = sqlx::query("SELECT recording_id, checksum, evidence_kind, evidence_digest FROM observations WHERE locator = ?")
        .bind(&track.locator).fetch_optional(&mut *connection).await.map_err(state_error)?;
    let Some(previous) = previous else {
        return Ok(None);
    };
    let checksum: String = previous.get(1);
    let kind: Option<String> = previous.get(2);
    let evidence: Option<String> = previous.get(3);
    let verified_audio = track.evidence.as_ref().is_some_and(|current| {
        kind.as_deref() == Some(pcm::ALGORITHM) && evidence.as_ref() == Some(current)
    });
    if checksum == track.checksum || verified_audio {
        Ok(Some(previous.get(0)))
    } else {
        Ok(None)
    }
}

async fn historical(
    connection: &mut SqliteConnection,
    track: &Track,
) -> io::Result<Option<String>> {
    let exact = candidates(connection, "file-v1", &track.checksum).await?;
    if exact.len() == 1 {
        return Ok(exact.into_iter().next());
    }
    if !exact.is_empty() {
        return Ok(None);
    }
    let Some(evidence) = &track.evidence else {
        return Ok(None);
    };
    let audio = candidates(connection, pcm::ALGORITHM, evidence).await?;
    Ok(if audio.len() == 1 {
        audio.into_iter().next()
    } else {
        None
    })
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
