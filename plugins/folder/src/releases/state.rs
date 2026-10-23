use crate::catalog::Track;
use futures_util::TryStreamExt;
use hito_plugin_sdk::ProviderRecording;
use sqlx::{Row, SqliteConnection};
use std::{
    collections::{BTreeMap, BTreeSet},
    io,
};

pub(super) struct Prior {
    pub memberships: BTreeMap<String, BTreeSet<String>>,
    pub occurrences: BTreeMap<Vec<u8>, Occurrence>,
}

pub(super) struct Occurrence {
    pub release: String,
    pub recording: String,
    pub checksum: String,
    pub evidence_kind: Option<String>,
    pub evidence: Option<String>,
}

pub(crate) async fn create(connection: &mut SqliteConnection) -> io::Result<()> {
    sqlx::query("CREATE TABLE releases (id TEXT PRIMARY KEY NOT NULL CHECK(length(CAST(id AS BLOB)) BETWEEN 1 AND 256), eligible INTEGER NOT NULL CHECK(eligible IN (0, 1)))")
        .execute(&mut *connection).await.map_err(database_error)?;
    sqlx::query("CREATE TABLE release_memberships (release_id TEXT NOT NULL REFERENCES releases(id), recording_id TEXT NOT NULL REFERENCES recordings(id), witness INTEGER NOT NULL CHECK(witness IN (0, 1)), PRIMARY KEY(release_id, recording_id))")
        .execute(&mut *connection).await.map_err(database_error)?;
    sqlx::query("ALTER TABLE observations ADD COLUMN release_id TEXT REFERENCES releases(id)")
        .execute(&mut *connection)
        .await
        .map_err(database_error)?;
    observation_limits(connection).await?;
    let mut bindings = Vec::new();
    for (table, id_column, locator_column) in [
        ("recordings", "id", "NULL"),
        ("observations", "recording_id", "locator"),
    ] {
        let limits = format!(
            "SELECT count(*), coalesce(sum(length(CAST(detail AS BLOB))), 0), coalesce(max(length(CAST(detail AS BLOB))), 0), coalesce(max(length(CAST({id_column} AS BLOB))), 0) FROM {table}"
        );
        let row = sqlx::query(&limits)
            .fetch_one(&mut *connection)
            .await
            .map_err(database_error)?;
        let count: i64 = row.try_get(0).map_err(database_error)?;
        let bytes: i64 = row.try_get(1).map_err(database_error)?;
        let maximum: i64 = row.try_get(2).map_err(database_error)?;
        let maximum_id: i64 = row.try_get(3).map_err(database_error)?;
        if count > 10000 || bytes > 32 * 1024 * 1024 || maximum > 32768 || maximum_id > 256 {
            return Err(io::Error::other("Folder release migration limit exceeded"));
        }
        let query = format!("SELECT {id_column}, {locator_column}, detail FROM {table}");
        let mut rows = sqlx::query(&query).fetch(&mut *connection);
        while let Some(row) = rows.try_next().await.map_err(database_error)? {
            let id: String = row.try_get(0).map_err(database_error)?;
            let locator: Option<Vec<u8>> = row.try_get(1).map_err(database_error)?;
            let detail: String = row.try_get(2).map_err(database_error)?;
            let recording: ProviderRecording = serde_json::from_str(&detail)
                .map_err(|_| io::Error::other("Invalid folder release metadata"))?;
            recording
                .validate()
                .map_err(|_| io::Error::other("Invalid folder release metadata"))?;
            if recording.id != id {
                return Err(io::Error::other("Folder release recording binding changed"));
            }
            if let Some(release) = recording.release {
                bindings.push((id, release.id, locator));
            }
        }
    }
    for (recording, release, locator) in bindings {
        let witness = i64::from(locator.is_some());
        sqlx::query("INSERT INTO releases VALUES (?, ?) ON CONFLICT(id) DO UPDATE SET eligible = max(releases.eligible, excluded.eligible)")
            .bind(&release).bind(witness).execute(&mut *connection).await.map_err(database_error)?;
        sqlx::query("INSERT INTO release_memberships VALUES (?, ?, ?) ON CONFLICT(release_id, recording_id) DO UPDATE SET witness = max(release_memberships.witness, excluded.witness)")
            .bind(&release).bind(recording).bind(witness).execute(&mut *connection).await.map_err(database_error)?;
        if let Some(locator) = locator {
            sqlx::query("UPDATE observations SET release_id = ? WHERE locator = ?")
                .bind(release)
                .bind(locator)
                .execute(&mut *connection)
                .await
                .map_err(database_error)?;
        }
    }
    check_limits(connection).await
}

pub(super) async fn load(connection: &mut SqliteConnection) -> io::Result<Prior> {
    check_limits(connection).await?;
    observation_limits(connection).await?;
    let mut memberships = BTreeMap::<String, BTreeSet<String>>::new();
    let mut rows = sqlx::query("SELECT m.recording_id, m.release_id FROM release_memberships m JOIN releases r ON r.id = m.release_id WHERE r.eligible = 1 AND m.witness = 1")
        .fetch(&mut *connection);
    while let Some(row) = rows.try_next().await.map_err(database_error)? {
        let recording: String = row.try_get(0).map_err(database_error)?;
        let release: String = row.try_get(1).map_err(database_error)?;
        memberships.entry(recording).or_default().insert(release);
    }
    drop(rows);
    let mut occurrences = BTreeMap::new();
    let mut rows = sqlx::query("SELECT o.locator, o.recording_id, o.release_id, o.checksum, o.evidence_kind, o.evidence_digest FROM observations o JOIN releases r ON r.id = o.release_id WHERE r.eligible = 1")
        .fetch(&mut *connection);
    while let Some(row) = rows.try_next().await.map_err(database_error)? {
        let locator: Vec<u8> = row.try_get(0).map_err(database_error)?;
        let occurrence = Occurrence {
            recording: row.try_get(1).map_err(database_error)?,
            release: row.try_get(2).map_err(database_error)?,
            checksum: row.try_get(3).map_err(database_error)?,
            evidence_kind: row.try_get(4).map_err(database_error)?,
            evidence: row.try_get(5).map_err(database_error)?,
        };
        if !memberships
            .get(&occurrence.recording)
            .is_some_and(|releases| releases.contains(&occurrence.release))
        {
            return Err(io::Error::other("Invalid folder release witness"));
        }
        occurrences.insert(locator, occurrence);
    }
    Ok(Prior {
        memberships,
        occurrences,
    })
}

pub(super) async fn allocate(
    connection: &mut SqliteConnection,
    allocated: &BTreeSet<String>,
    retired: &BTreeSet<String>,
) -> io::Result<()> {
    for release in retired {
        let updated = sqlx::query("UPDATE releases SET eligible = 0 WHERE id = ? AND eligible = 1")
            .bind(release)
            .execute(&mut *connection)
            .await
            .map_err(database_error)?;
        if updated.rows_affected() != 1 {
            return Err(io::Error::other("Folder release eligibility changed"));
        }
    }
    for release in allocated {
        sqlx::query("INSERT INTO releases VALUES (?, 1)")
            .bind(release)
            .execute(&mut *connection)
            .await
            .map_err(database_error)?;
    }
    check_limits(connection).await
}

pub(crate) async fn observe(connection: &mut SqliteConnection, tracks: &[Track]) -> io::Result<()> {
    let mut groups = BTreeMap::<String, BTreeSet<String>>::new();
    for track in tracks {
        if let Some(release) = &track.release_id {
            groups
                .entry(release.clone())
                .or_default()
                .insert(track.id.clone());
        }
    }
    for (release, recordings) in groups {
        sqlx::query(
            "UPDATE release_memberships SET witness = 0 WHERE release_id = ? AND witness = 1",
        )
        .bind(&release)
        .execute(&mut *connection)
        .await
        .map_err(database_error)?;
        for recording in recordings {
            sqlx::query("INSERT INTO release_memberships VALUES (?, ?, 1) ON CONFLICT(release_id, recording_id) DO UPDATE SET witness = 1")
                .bind(&release).bind(recording).execute(&mut *connection).await.map_err(database_error)?;
        }
    }
    check_limits(connection).await
}

async fn check_limits(connection: &mut SqliteConnection) -> io::Result<()> {
    for query in [
        "SELECT count(*), coalesce(sum(length(CAST(id AS BLOB))), 0), coalesce(max(length(CAST(id AS BLOB))), 0), 0 FROM releases",
        "SELECT count(*), coalesce(sum(length(CAST(release_id AS BLOB)) + length(CAST(recording_id AS BLOB))), 0), coalesce(max(length(CAST(release_id AS BLOB))), 0), coalesce(max(length(CAST(recording_id AS BLOB))), 0) FROM release_memberships",
    ] {
        let row = sqlx::query(query)
            .fetch_one(&mut *connection)
            .await
            .map_err(database_error)?;
        let count: i64 = row.try_get(0).map_err(database_error)?;
        let bytes: i64 = row.try_get(1).map_err(database_error)?;
        let maximum_release: i64 = row.try_get(2).map_err(database_error)?;
        let maximum_recording: i64 = row.try_get(3).map_err(database_error)?;
        if count > 100000
            || bytes > 32 * 1024 * 1024
            || maximum_release > 256
            || maximum_recording > 256
        {
            return Err(io::Error::other("Folder release history limit exceeded"));
        }
    }
    Ok(())
}

async fn observation_limits(connection: &mut SqliteConnection) -> io::Result<()> {
    let row = sqlx::query("SELECT count(*), coalesce(sum(length(locator) + length(CAST(recording_id || coalesce(release_id, '') || checksum || coalesce(evidence_kind, '') || coalesce(evidence_digest, '') AS BLOB))), 0), coalesce(max(length(locator)), 0), coalesce(max(length(CAST(recording_id || coalesce(release_id, '') || checksum || coalesce(evidence_kind, '') || coalesce(evidence_digest, '') AS BLOB))), 0) FROM observations")
        .fetch_one(&mut *connection).await.map_err(database_error)?;
    let count: i64 = row.try_get(0).map_err(database_error)?;
    let bytes: i64 = row.try_get(1).map_err(database_error)?;
    let maximum_locator: i64 = row.try_get(2).map_err(database_error)?;
    let maximum_identity: i64 = row.try_get(3).map_err(database_error)?;
    if count > 10000
        || bytes > 32 * 1024 * 1024
        || maximum_locator > 16384
        || maximum_identity > 1024
    {
        return Err(io::Error::other(
            "Folder release observation limit exceeded",
        ));
    }
    Ok(())
}

fn database_error(_: sqlx::Error) -> io::Error {
    io::Error::other("Folder release state operation failed")
}
