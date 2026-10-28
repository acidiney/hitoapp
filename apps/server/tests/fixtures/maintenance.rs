use sha2::{Digest, Sha256};
use sqlx::{Connection, Row, SqliteConnection, migrate::Migrator, sqlite::SqliteConnectOptions};
use std::{
    borrow::Cow,
    collections::BTreeMap,
    path::{Path, PathBuf},
};
use uuid::Uuid;

pub use hito_core::test_support::migrations;

pub fn id(value: u128) -> String {
    Uuid::from_u128(0x10000000_0000_4000_8000_000000000000 + value).to_string()
}

pub async fn connect(path: &Path) -> SqliteConnection {
    SqliteConnection::connect_with(
        &SqliteConnectOptions::new()
            .filename(path)
            .create_if_missing(false)
            .foreign_keys(true),
    )
    .await
    .unwrap()
}

pub struct Library {
    pub directory: tempfile::TempDir,
    pub database: PathBuf,
}

impl Library {
    pub async fn version_29(endpoint: &str) -> Self {
        let directory = tempfile::tempdir().unwrap();
        let database = directory.path().join("hito.db");
        let mut connection = SqliteConnection::connect_with(
            &SqliteConnectOptions::new()
                .filename(&database)
                .create_if_missing(true)
                .foreign_keys(true)
                .journal_mode(sqlx::sqlite::SqliteJournalMode::Wal),
        )
        .await
        .unwrap();
        let migrations = migrations();
        Migrator {
            migrations: Cow::Owned(
                migrations
                    .iter()
                    .filter(|migration| migration.version <= 29)
                    .cloned()
                    .collect(),
            ),
            ..Migrator::DEFAULT
        }
        .run(&mut connection)
        .await
        .unwrap();
        sqlx::query("INSERT INTO users(id,username,password_hash,admin) VALUES ('account','owned-maintenance','unused',1)")
            .execute(&mut connection).await.unwrap();
        sqlx::raw_sql("INSERT INTO libraries(id,name,owner_id) VALUES ('provider-library','Owned provider library','account'),('folder-library','Owned original library',NULL);
            INSERT INTO library_members(library_id,user_id) VALUES ('provider-library','account'),('folder-library','account');")
            .execute(&mut connection).await.unwrap();
        sqlx::query("INSERT INTO plugin_services(id,name,base_url) VALUES ('service','Owned maintenance provider',?)")
            .bind(endpoint).execute(&mut connection).await.unwrap();
        let key = [37_u8; 32];
        let mut credential = vec![1];
        credential.extend_from_slice(&Sha256::digest(key));
        credential.extend_from_slice(&[19; 48]);
        let mut digest = Sha256::new();
        digest.update(b"some");
        digest.update(&credential);
        let credential_digest = format!("{:x}", digest.finalize());
        sqlx::query("INSERT INTO plugin_installations(id,user_id,service_id,manifest,credential) VALUES ('installation','account','service',?,?)")
            .bind(r#"{"id":"org.hito.maintenance","name":"Owned maintenance provider","protocol_version":1,"capabilities":["sources","playlists"]}"#)
            .bind(&credential).execute(&mut connection).await.unwrap();
        let music = directory.path().join("owned-music");
        std::fs::create_dir(&music).unwrap();
        let audio = audio();
        std::fs::write(music.join("original.wav"), &audio).unwrap();
        sqlx::query("INSERT INTO music_folders(id,library_id,host_device,directory_id,root_path,name,connected,bound) VALUES ('folder','folder-library','owned-host','owned-directory',?,'Owned music',1,1)")
            .bind(music.to_string_lossy().as_ref()).execute(&mut connection).await.unwrap();
        sqlx::query("INSERT INTO jobs(id,user_id,library_id,folder_id,folder_generation,kind,state) VALUES ('scan','account','folder-library','folder',1,'scan','queued')")
            .execute(&mut connection).await.unwrap();
        sqlx::query("INSERT INTO playlists(id,user_id,name,kind) VALUES (?,'account','Owned Follow','followed')")
            .bind(id(5)).execute(&mut connection).await.unwrap();
        for (index, state) in ["queued", "failed", "ready", "removed"].iter().enumerate() {
            let recording = id(10 + index as u128);
            let source = id(20 + index as u128);
            let binding = id(30 + index as u128);
            let transfer = id(100 + index as u128);
            sqlx::query("INSERT INTO recordings(id,library_id,title,artist,provenance) VALUES (?,'provider-library','Owned maintenance recording','Owned artist','provider_response')")
                .bind(&recording).execute(&mut connection).await.unwrap();
            sqlx::query(
                "INSERT INTO sources(id,recording_id,provider,provider_id) VALUES (?,?,'plugin',?)",
            )
            .bind(&source)
            .bind(&recording)
            .bind(format!("owned-source-{index}"))
            .execute(&mut connection)
            .await
            .unwrap();
            sqlx::query("INSERT INTO provider_items(id,installation_id,provider_recording_id,recording_id,role,evidence) VALUES (?,'installation',?,?,'primary','provider_identity')")
                .bind(&binding).bind(format!("owned-recording-{index}")).bind(&recording).execute(&mut connection).await.unwrap();
            sqlx::query("INSERT INTO metadata_observations(id,recording_id,provider,origin,title,artist,duration_ms,provider_binding_id) VALUES (?,?,'org.hito.maintenance','provider_response','Owned maintenance recording','Owned artist',1000,?)")
                .bind(id(40 + index as u128)).bind(&recording).bind(&binding).execute(&mut connection).await.unwrap();
            sqlx::query("INSERT INTO provider_source_state(binding_id,generation,observation_id) VALUES (?,0,?)")
                .bind(&binding).bind(id(40 + index as u128)).execute(&mut connection).await.unwrap();
            sqlx::query("INSERT INTO provider_sources(source_id,binding_id,installation_id,recording_id,provider_source_id,label,generation) VALUES (?,?,'installation',?,?,'Owned audio',0)")
                .bind(&source).bind(&binding).bind(&recording).bind(format!("owned-source-{index}")).execute(&mut connection).await.unwrap();
            if *state == "ready" {
                sqlx::query("INSERT INTO sources(id,recording_id,provider,provider_id,device_id) VALUES (?,?,'download',?,'owned-host')")
                    .bind(id(200)).bind(&recording).bind(id(201)).execute(&mut connection).await.unwrap();
                sqlx::query("INSERT INTO download_files(id,account_id,host_device,recording_id,source_id,state,attempt_id,checksum,byte_length,mime) VALUES (?,'account','owned-host',?,?,'ready',?,?,?,'audio/wav')")
                    .bind(id(201)).bind(&recording).bind(id(200)).bind(id(202)).bind(format!("{:x}", Sha256::digest(&audio))).bind(audio.len() as i64).execute(&mut connection).await.unwrap();
            }
            sqlx::query("INSERT INTO download_intents(rowid,id,account_id,recording_id,library_id,access_grant_id,host_device,state,file_id,provider_source_id,binding_id,binding_revision,remote_source_id,installation_id,installation_revision,service_id,service_revision,credential_digest,audio_generation,error)
                SELECT ?,?,'account',?,'provider-library',id,'owned-host',?,?, ?,?,1,?,'installation',1,'service',1,?,0,? FROM library_access_grants WHERE library_id='provider-library' AND user_id='account'")
                .bind(31 + index as i64 * 127).bind(&transfer).bind(&recording).bind(*state).bind((*state == "ready").then(|| id(201))).bind(&source).bind(&binding).bind(format!("owned-source-{index}"))
                .bind(&credential_digest).bind((*state == "failed").then_some("transfer_failed")).execute(&mut connection).await.unwrap();
            if *state == "ready" {
                sqlx::query(
                    "UPDATE download_intents SET bytes_received=?,total_bytes=? WHERE id=?",
                )
                .bind(audio.len() as i64)
                .bind(audio.len() as i64)
                .bind(&transfer)
                .execute(&mut connection)
                .await
                .unwrap();
            }
            sqlx::query("INSERT INTO download_jobs(id,account_id,intent_id,generation,status,next_attempt_at) VALUES (?,'account',?,1,?,0)")
                .bind(id(110 + index as u128)).bind(&transfer).bind(match *state { "queued" => "queued", "failed" => "failed", "ready" => "complete", _ => "cancelled" }).execute(&mut connection).await.unwrap();
            sqlx::query("INSERT INTO download_receipts(rowid,account_id,request_id,request_digest,intent_id) VALUES (?,'account',?,?,?)")
                .bind(17 + index as i64 * 113).bind(id(120 + index as u128)).bind("b".repeat(64)).bind(&transfer).execute(&mut connection).await.unwrap();
            sqlx::query("INSERT INTO playlist_entries(playlist_id,id,recording_id,position) VALUES (?,?,?,?)")
                .bind(id(5)).bind(id(130 + index as u128)).bind(&recording).bind(index as i64).execute(&mut connection).await.unwrap();
        }
        sqlx::query("INSERT INTO playlist_follows(playlist_id,user_id,installation_id,installation_revision,service_id,service_revision,snapshot_installation_revision,snapshot_service_revision,provider,provider_playlist_id,snapshot,content_digest,attribution,interval_seconds,next_refresh_at) VALUES (?,'account','installation',1,'service',1,1,1,'org.hito.maintenance','owned-playlist','owned-snapshot',?,'{}',3600,0)")
            .bind(id(5)).bind("c".repeat(64)).execute(&mut connection).await.unwrap();
        sqlx::query("INSERT INTO playlist_follow_jobs(id,user_id,playlist_id,generation,status,next_attempt_at) VALUES ('follow-job','account',?,1,'queued',0)")
            .bind(id(5)).execute(&mut connection).await.unwrap();
        let operation = hito_core::Operation {
            id: Uuid::parse_str(&id(300)).unwrap(),
            action: hito_core::Action::PlaylistRename {
                playlist_id: Uuid::parse_str(&id(5)).unwrap(),
                name: "Owned Follow".into(),
            },
        };
        sqlx::query("INSERT INTO sync_events(sequence,user_id,origin,device_id,event_id,payload) VALUES (7,'account','client_operation','owned-sync-client',?,?)")
            .bind(operation.id.to_string()).bind(serde_json::to_string(&operation).unwrap()).execute(&mut connection).await.unwrap();
        sqlx::query("INSERT INTO sync_events(sequence,user_id,origin,device_id,event_id,payload) VALUES (900000,'account','client_operation','owned-sync-client',?,'{}')")
            .bind(id(301)).execute(&mut connection).await.unwrap();
        sqlx::query("DELETE FROM sync_events WHERE sequence=900000")
            .execute(&mut connection)
            .await
            .unwrap();
        sqlx::query("INSERT INTO likes(user_id,recording_id,liked) VALUES ('account',?,1)")
            .bind(id(10))
            .execute(&mut connection)
            .await
            .unwrap();
        sqlx::query("INSERT INTO listening_progress(user_id,recording_id,position_ms,speed) VALUES ('account',?,700,1.25)")
            .bind(id(10)).execute(&mut connection).await.unwrap();
        assert_eq!(
            sqlx::query_scalar::<_, i64>("SELECT max(version) FROM _sqlx_migrations")
                .fetch_one(&mut connection)
                .await
                .unwrap(),
            29
        );
        assert_eq!(
            sqlx::query_scalar::<_, i64>("SELECT count(*) FROM current_download_authority")
                .fetch_one(&mut connection)
                .await
                .unwrap(),
            1
        );
        assert_eq!(
            sqlx::query_scalar::<_, i64>("SELECT count(*) FROM current_playlist_follow_authority")
                .fetch_one(&mut connection)
                .await
                .unwrap(),
            1
        );
        assert_eq!(
            sqlx::query_scalar::<_, i64>(
                "SELECT seq FROM sqlite_sequence WHERE name='sync_events'"
            )
            .fetch_one(&mut connection)
            .await
            .unwrap(),
            900000
        );
        assert_eq!(
            sqlx::query_scalar::<_, i64>("SELECT max(sequence) FROM sync_events")
                .fetch_one(&mut connection)
                .await
                .unwrap(),
            7
        );
        assert_eq!(
            sqlx::query_scalar::<_, i64>("SELECT rowid FROM download_receipts ORDER BY rowid")
                .fetch_all(&mut connection)
                .await
                .unwrap(),
            vec![17, 130, 243, 356]
        );
        assert!(
            sqlx::query("PRAGMA foreign_key_check")
                .fetch_all(&mut connection)
                .await
                .unwrap()
                .is_empty()
        );
        connection.close().await.unwrap();
        std::fs::write(database.with_extension("plugin-key"), key).unwrap();
        std::fs::write(database.with_extension("device-id"), "owned-host").unwrap();
        let managed = directory.path().join("hito.downloads");
        std::fs::create_dir(&managed).unwrap();
        let media = managed.join(format!("{}.{}.media", id(201), id(202)));
        std::fs::write(&media, audio).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&managed, std::fs::Permissions::from_mode(0o700)).unwrap();
            for path in [
                database.with_extension("plugin-key"),
                database.with_extension("device-id"),
                media,
            ] {
                std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600)).unwrap();
            }
        }
        Self {
            directory,
            database,
        }
    }

    pub fn files(&self) -> BTreeMap<PathBuf, Vec<u8>> {
        fn visit(root: &Path, directory: &Path, files: &mut BTreeMap<PathBuf, Vec<u8>>) {
            for entry in std::fs::read_dir(directory).unwrap() {
                let entry = entry.unwrap();
                let path = entry.path();
                if entry.file_type().unwrap().is_dir() {
                    files.insert(path.strip_prefix(root).unwrap().to_owned(), Vec::new());
                    visit(root, &path, files);
                } else if !["hito.db", "hito.db-wal", "hito.db-shm"]
                    .contains(&entry.file_name().to_str().unwrap())
                {
                    files.insert(
                        path.strip_prefix(root).unwrap().to_owned(),
                        std::fs::read(path).unwrap(),
                    );
                }
            }
        }
        let mut files = BTreeMap::new();
        visit(self.directory.path(), self.directory.path(), &mut files);
        files
    }
}

fn audio() -> Vec<u8> {
    let mut bytes = b"RIFF".to_vec();
    bytes.extend_from_slice(&68_u32.to_le_bytes());
    bytes.extend_from_slice(b"WAVEfmt ");
    bytes.extend_from_slice(&16_u32.to_le_bytes());
    bytes.extend_from_slice(&1_u16.to_le_bytes());
    bytes.extend_from_slice(&1_u16.to_le_bytes());
    bytes.extend_from_slice(&8000_u32.to_le_bytes());
    bytes.extend_from_slice(&16000_u32.to_le_bytes());
    bytes.extend_from_slice(&2_u16.to_le_bytes());
    bytes.extend_from_slice(&16_u16.to_le_bytes());
    bytes.extend_from_slice(b"data");
    bytes.extend_from_slice(&32_u32.to_le_bytes());
    bytes.extend_from_slice(&[0; 32]);
    bytes
}

#[derive(Clone, PartialEq, Eq)]
enum Cell {
    Null,
    Integer(i64),
    Real(u64),
    Text(String),
    Blob(Vec<u8>),
}

struct Table {
    name: String,
    columns: Vec<String>,
    has_rowid: bool,
    rows: Vec<Vec<Cell>>,
}

pub struct Snapshot {
    tables: Vec<Table>,
}

impl Snapshot {
    pub async fn read(path: &Path) -> Self {
        let mut connection = connect(path).await;
        let names: Vec<(String, bool)> = sqlx::query_as("SELECT name,wr=0 FROM pragma_table_list WHERE schema='main' AND type IN ('table','shadow','virtual') AND (name NOT LIKE 'sqlite_%' OR name='sqlite_sequence') ORDER BY name")
            .fetch_all(&mut connection).await.unwrap();
        let mut tables = Vec::new();
        for (name, has_rowid) in names {
            let columns: Vec<String> = sqlx::query_scalar(
                "SELECT name FROM pragma_table_xinfo(?) WHERE hidden=0 ORDER BY cid",
            )
            .bind(&name)
            .fetch_all(&mut connection)
            .await
            .unwrap();
            let rows = rows(&mut connection, &name, &columns, has_rowid).await;
            tables.push(Table {
                name,
                columns,
                has_rowid,
                rows,
            });
        }
        connection.close().await.unwrap();
        Self { tables }
    }

    pub async fn assert_preserved(&self, path: &Path) {
        let mut connection = connect(path).await;
        for table in &self.tables {
            let has_rowid: bool = sqlx::query_scalar(
                "SELECT wr=0 FROM pragma_table_list WHERE schema='main' AND name=?",
            )
            .bind(&table.name)
            .fetch_one(&mut connection)
            .await
            .unwrap();
            assert_eq!(
                has_rowid, table.has_rowid,
                "maintenance changed rowid ownership in {}",
                table.name
            );
            let mut actual = rows(
                &mut connection,
                &table.name,
                &table.columns,
                table.has_rowid,
            )
            .await;
            if table.name == "_sqlx_migrations" || table.name == "sqlite_sequence" {
                assert!(
                    table.has_rowid,
                    "migration and sequence ledgers must retain their hidden rowids"
                );
                actual.retain(|row| table.rows.iter().any(|old| row[1] == old[1]));
            }
            assert!(
                actual == table.rows,
                "maintenance changed prior typed rows or rowids in {}",
                table.name
            );
            let current: Vec<String> = sqlx::query_scalar(
                "SELECT name FROM pragma_table_xinfo(?) WHERE hidden=0 ORDER BY cid",
            )
            .bind(&table.name)
            .fetch_all(&mut connection)
            .await
            .unwrap();
            let mut expected = table.columns.clone();
            if table.name == "download_receipts" {
                expected.push("retention_id".into());
            }
            if table.name == "download_intents" {
                let origin_columns = [
                    "origin_kind",
                    "podcast_source_id",
                    "podcast_episode_id",
                    "podcast_subscription_id",
                    "podcast_subscription_generation",
                    "podcast_enclosure_revision",
                    "podcast_enclosure_url",
                ]
                .map(str::to_owned);
                expected.extend_from_slice(&origin_columns);
                assert!(
                    table.has_rowid,
                    "legacy download intents must retain rowids"
                );
                let origins = rows(&mut connection, &table.name, &origin_columns, true).await;
                let expected_origins: Vec<Vec<Cell>> = table
                    .rows
                    .iter()
                    .map(|row| {
                        vec![
                            row[0].clone(),
                            Cell::Text("provider".into()),
                            Cell::Null,
                            Cell::Null,
                            Cell::Null,
                            Cell::Null,
                            Cell::Null,
                            Cell::Null,
                        ]
                    })
                    .collect();
                assert!(
                    origins == expected_origins,
                    "maintenance changed legacy provider origin types, values, or rowids"
                );
            }
            if table.name == "artwork_fetches" {
                let target_columns = [
                    "entity_kind",
                    "release_id",
                    "provider_release_id",
                    "playlist_id",
                    "provider_playlist_id",
                    "playlist_provider",
                    "playlist_snapshot",
                    "playlist_imported_at",
                    "retained",
                ]
                .map(str::to_owned);
                expected.extend_from_slice(&target_columns);
                assert!(table.has_rowid, "legacy artwork jobs must retain rowids");
                assert_eq!(table.columns.len(), 20);
                let job_id = table.columns.iter().position(|name| name == "id").unwrap() + 1;
                let references = self
                    .tables
                    .iter()
                    .find(|table| table.name == "recording_artwork")
                    .unwrap();
                let fetch_id = references
                    .columns
                    .iter()
                    .position(|name| name == "fetch_id")
                    .unwrap()
                    + usize::from(references.has_rowid);
                let targets = rows(&mut connection, &table.name, &target_columns, true).await;
                let expected_targets: Vec<Vec<Cell>> = table
                    .rows
                    .iter()
                    .map(|row| {
                        let retained = references
                            .rows
                            .iter()
                            .any(|reference| reference[fetch_id] == row[job_id]);
                        vec![
                            row[0].clone(),
                            Cell::Text("recording".into()),
                            Cell::Null,
                            Cell::Null,
                            Cell::Null,
                            Cell::Null,
                            Cell::Null,
                            Cell::Null,
                            Cell::Null,
                            Cell::Integer(i64::from(retained)),
                        ]
                    })
                    .collect();
                assert!(
                    targets == expected_targets,
                    "maintenance changed legacy artwork identity, derived retention, types, or rowids"
                );
            }
            assert!(
                current == expected,
                "maintenance unexpectedly changed prior columns in {}",
                table.name
            );
        }
        assert_current_schema(&mut connection).await;
        connection.close().await.unwrap();
    }
}

async fn assert_current_schema(connection: &mut SqliteConnection) {
    type SchemaObject = (String, String, String, Option<String>);
    type MigrationReceipt = (i64, String, bool, Vec<u8>);
    let mut reference = SqliteConnection::connect_with(
        &SqliteConnectOptions::new()
            .in_memory(true)
            .foreign_keys(true),
    )
    .await
    .unwrap();
    let schema = "SELECT type,name,tbl_name,sql FROM sqlite_schema ORDER BY type,name";
    let all = migrations();
    Migrator {
        migrations: Cow::Owned(
            all.iter()
                .filter(|migration| migration.version <= 52)
                .cloned()
                .collect(),
        ),
        ..Migrator::DEFAULT
    }
    .run(&mut reference)
    .await
    .unwrap();
    let prior: Vec<SchemaObject> = sqlx::query_as(schema)
        .fetch_all(&mut reference)
        .await
        .unwrap();
    all.run(&mut reference).await.unwrap();
    let actual: Vec<SchemaObject> = sqlx::query_as(schema)
        .fetch_all(&mut *connection)
        .await
        .unwrap();
    let expected: Vec<SchemaObject> = sqlx::query_as(schema)
        .fetch_all(&mut reference)
        .await
        .unwrap();
    for object in &prior {
        let mut declared = object.clone();
        if declared.0 == "table" && declared.1 == "source_resolutions" {
            declared.3 = declared.3.map(|ddl| {
                ddl.replace(
                    "'no_suitable_source'",
                    "'no_suitable_source','provider_unavailable'",
                )
            });
        }
        assert!(
            expected.contains(&declared),
            "later migrations must retain every exact historical 0052 schema object except the declared source resolution outcome set"
        );
    }
    let mut added = expected
        .iter()
        .filter(|object| {
            !prior
                .iter()
                .any(|old| old.0 == object.0 && old.1 == object.1)
        })
        .map(|object| (object.0.as_str(), object.1.as_str()))
        .collect::<Vec<_>>();
    let mut declared_additions = vec![
        ("table", "account_community_ranking_preferences"),
        (
            "index",
            "sqlite_autoindex_account_community_ranking_preferences_1",
        ),
        ("table", "account_artist_identity_groups"),
        ("index", "sqlite_autoindex_account_artist_identity_groups_1"),
        ("index", "sqlite_autoindex_account_artist_identity_groups_2"),
        ("index", "sqlite_autoindex_account_artist_identity_groups_3"),
        ("table", "recording_artist_identity_choices"),
        (
            "index",
            "sqlite_autoindex_recording_artist_identity_choices_1",
        ),
        ("index", "recording_artist_identity_group"),
        ("index", "recording_artist_identity_recording"),
        ("table", "artist_identity_choice_receipts"),
        (
            "index",
            "sqlite_autoindex_artist_identity_choice_receipts_1",
        ),
        ("trigger", "artist_identity_receipt_immutable"),
        ("trigger", "artist_identity_receipt_retained"),
        ("table", "album_edition_choices"),
        ("index", "sqlite_autoindex_album_edition_choices_1"),
        ("trigger", "album_edition_choice_scope_insert"),
        ("trigger", "album_edition_choice_scope_update"),
        ("table", "album_edition_choice_receipts"),
        ("index", "sqlite_autoindex_album_edition_choice_receipts_1"),
        ("trigger", "album_edition_receipt_immutable"),
        ("trigger", "album_edition_receipt_retained"),
        ("table", "recording_artist_association_changes"),
        (
            "index",
            "sqlite_autoindex_recording_artist_association_changes_1",
        ),
        ("index", "recording_artist_association_change_association"),
        ("trigger", "recording_artist_association_change_scope"),
        ("trigger", "recording_artist_association_change_immutable"),
        ("trigger", "recording_artist_association_change_retained"),
        ("table", "recording_artist_association_receipts"),
        (
            "index",
            "sqlite_autoindex_recording_artist_association_receipts_1",
        ),
        ("trigger", "recording_artist_association_receipt_immutable"),
        ("trigger", "recording_artist_association_receipt_retained"),
        ("table", "account_pin_locks"),
        ("index", "sqlite_autoindex_account_pin_locks_1"),
        ("table", "session_locks"),
        ("index", "sqlite_autoindex_session_locks_1"),
        ("index", "recording_artist_association_recording"),
        ("table", "artist_identity_changes"),
        ("index", "sqlite_autoindex_artist_identity_changes_1"),
        ("index", "sqlite_autoindex_artist_identity_changes_2"),
        ("trigger", "artist_identity_change_immutable"),
        ("trigger", "artist_identity_change_retained"),
        ("table", "artist_identity_change_items"),
        ("index", "sqlite_autoindex_artist_identity_change_items_1"),
        ("index", "artist_identity_change_item_recording"),
        ("trigger", "artist_identity_change_item_immutable"),
        ("trigger", "artist_identity_change_item_retained"),
        ("table", "artist_association_change_retirements"),
        ("index", "sqlite_autoindex_artist_association_change_retirements_1"),
        ("trigger", "artist_association_change_retirement_immutable"),
        ("trigger", "artist_association_change_retirement_retained"),
        ("table", "shared_artist_corrections"),
        ("index", "sqlite_autoindex_shared_artist_corrections_1"),
        ("trigger", "shared_artist_correction_immutable"),
        ("trigger", "shared_artist_correction_retained"),
        ("trigger", "shared_artist_correction_administrator"),
        ("table", "shared_artist_correction_subjects"),
        ("index", "sqlite_autoindex_shared_artist_correction_subjects_1"),
        ("index", "shared_artist_correction_subject_correction"),
        ("trigger", "shared_artist_correction_subject_immutable"),
        ("trigger", "shared_artist_correction_subject_retained"),
        ("table", "shared_artist_correction_states"),
        ("index", "sqlite_autoindex_shared_artist_correction_states_1"),
        ("trigger", "shared_artist_correction_state_immutable"),
        ("trigger", "shared_artist_correction_state_retained"),
        ("trigger", "shared_artist_correction_state_administrator"),
        ("table", "shared_artist_correction_receipts"),
        ("index", "sqlite_autoindex_shared_artist_correction_receipts_1"),
        ("trigger", "shared_artist_correction_receipt_immutable"),
        ("trigger", "shared_artist_correction_receipt_retained"),
    ];
    added.sort_unstable();
    declared_additions.sort_unstable();
    assert_eq!(
        added, declared_additions,
        "only the exact ranking preference, artist identity, album edition, artist association, PIN lock and artist correction objects may extend 0052"
    );
    assert!(
        actual == expected,
        "maintenance must retain every declared table, column, foreign key, check, unique constraint, index, trigger and view"
    );
    let ledger =
        "SELECT version,description,success,checksum FROM _sqlx_migrations ORDER BY version";
    let actual: Vec<MigrationReceipt> = sqlx::query_as(ledger)
        .fetch_all(&mut *connection)
        .await
        .unwrap();
    let expected: Vec<MigrationReceipt> = sqlx::query_as(ledger)
        .fetch_all(&mut reference)
        .await
        .unwrap();
    assert!(
        actual == expected,
        "maintenance must retain the complete current migration identities and successful checksums"
    );
    for table in [
        "account_community_preferences",
        "release_artwork",
        "playlist_artwork",
        "community_reporters",
        "community_reports",
        "account_community_ranking_preferences",
        "account_artist_identity_groups",
        "recording_artist_identity_choices",
        "artist_identity_choice_receipts",
        "album_edition_choices",
        "album_edition_choice_receipts",
        "recording_artist_association_changes",
        "recording_artist_association_receipts",
    ] {
        let count: i64 = sqlx::query_scalar(&format!("SELECT count(*) FROM {}", quoted(table)))
            .fetch_one(&mut *connection)
            .await
            .unwrap();
        assert_eq!(
            count, 0,
            "maintenance must not invent consent, community work, or collection covers"
        );
    }
    reference.close().await.unwrap();
}

fn quoted(identifier: &str) -> String {
    format!("\"{}\"", identifier.replace('"', "\"\""))
}

async fn rows(
    connection: &mut SqliteConnection,
    table: &str,
    columns: &[String],
    has_rowid: bool,
) -> Vec<Vec<Cell>> {
    let expressions = columns
        .iter()
        .map(|column| {
            let column = quoted(column);
            format!("typeof({column}),{column}")
        })
        .collect::<Vec<_>>()
        .join(",");
    let prefix = if has_rowid { "rowid," } else { "" };
    let ordering = if has_rowid {
        "rowid".to_owned()
    } else {
        columns
            .iter()
            .map(|column| {
                let column = quoted(column);
                format!("typeof({column}),{column} COLLATE BINARY")
            })
            .collect::<Vec<_>>()
            .join(",")
    };
    sqlx::query(&format!(
        "SELECT {prefix}{expressions} FROM {} ORDER BY {ordering}",
        quoted(table)
    ))
    .fetch_all(connection)
    .await
    .unwrap()
    .into_iter()
    .map(|row| {
        let offset = usize::from(has_rowid);
        let mut cells = if has_rowid {
            vec![Cell::Integer(row.get(0))]
        } else {
            Vec::new()
        };
        for index in 0..columns.len() {
            let storage = offset + index * 2;
            let value = storage + 1;
            cells.push(match row.get::<String, _>(storage).as_str() {
                "null" => Cell::Null,
                "integer" => Cell::Integer(row.get(value)),
                "real" => Cell::Real(row.get::<f64, _>(value).to_bits()),
                "text" => Cell::Text(row.get(value)),
                "blob" => Cell::Blob(row.get(value)),
                _ => panic!("unexpected SQLite storage class"),
            });
        }
        cells
    })
    .collect()
}
