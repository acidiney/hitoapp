#[path = "legacy_library.rs"]
mod legacy;
pub use legacy::{LIBRARY, Legacy, PLAYLIST, RECORDING, SOURCE};

use hito_core::ImportProviderPlaylist;
use serde_json::json;
use sha2::{Digest, Sha256};
use sqlx::{SqlitePool, migrate::Migrator, sqlite::SqliteConnectOptions};
use std::borrow::Cow;
use uuid::Uuid;

pub const LOCAL_RELEASE: &str = "30000000-0000-4000-8000-000000000001";
pub const PROVIDER_RECORDING: &str = "30000000-0000-4000-8000-000000000003";
pub const PROVIDER_RELEASE: &str = "30000000-0000-4000-8000-000000000004";
pub const PROVIDER_SOURCE: &str = "30000000-0000-4000-8000-000000000005";
pub const IMPORTED_PLAYLIST: &str = "30000000-0000-4000-8000-000000000006";
pub const INSTALLATION: &str = "release-credit-installation";
pub const OBSERVATION: &str = "30000000-0000-4000-8000-000000000010";
const PUBLICATION: &str = "30000000-0000-4000-8000-000000000011";
const METADATA: &str = r#"{"id":"recording/known","title":"Provider track","artist":"Provider performer","duration_ms":123000,"release":{"id":"album/known","title":"Provider album","artist":"Provider album credit"},"disc_number":1,"track_number":2,"revision":"known-one"}"#;

pub fn request() -> ImportProviderPlaylist {
    ImportProviderPlaylist {
        request_id: Uuid::parse_str("30000000-0000-4000-8000-000000000012").unwrap(),
        installation_revision: 1,
        playlist_reference: "https://provider.invalid/playlist/known".into(),
    }
}

pub async fn connect(fixture: &Legacy, foreign_keys: bool) -> SqlitePool {
    SqlitePool::connect_with(
        SqliteConnectOptions::new()
            .filename(fixture.directory.path().join("hito.db"))
            .foreign_keys(foreign_keys),
    )
    .await
    .unwrap()
}

async fn through(pool: &SqlitePool, version: i64) {
    let all = sqlx::migrate!("./migrations");
    Migrator {
        migrations: Cow::Owned(
            all.iter()
                .filter(|m| m.version <= version)
                .cloned()
                .collect(),
        ),
        ..Migrator::DEFAULT
    }
    .run(pool)
    .await
    .unwrap();
}

pub async fn seed(provider_url: &str) -> Legacy {
    let fixture = Legacy::seed().await;
    let pool = connect(&fixture, true).await;
    sqlx::query("INSERT INTO releases(rowid,id,library_id,title,artist,identity_key) VALUES (401,?,?,'Local album','Local album credit','local-album')")
        .bind(LOCAL_RELEASE).bind(LIBRARY).execute(&pool).await.unwrap();
    sqlx::query("UPDATE recordings SET release_id=? WHERE id=?")
        .bind(LOCAL_RELEASE)
        .bind(RECORDING)
        .execute(&pool)
        .await
        .unwrap();
    through(&pool, 25).await;
    sqlx::query("INSERT INTO plugin_services(id,name,base_url) VALUES ('release-credit-service','Owned provider',?)")
        .bind(provider_url).execute(&pool).await.unwrap();
    sqlx::raw_sql(include_str!("release_credits_upgrade.sql"))
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO metadata_observations(sequence,id,recording_id,release_id,provider,origin,observed_at,title,artist,album,album_artist,duration_ms,disc_number,track_number,plugin_installation_id,provider_recording_id,provider_revision,provider_metadata,provider_digest) VALUES (701,?,?,?,'org.hito.content-fixture','provider_response',123456,'Provider track','Provider performer','Provider album','Provider album credit',123000,1,2,?,'recording/known','known-one',?,?)")
        .bind(OBSERVATION).bind(PROVIDER_RECORDING).bind(PROVIDER_RELEASE).bind(INSTALLATION)
        .bind(METADATA).bind(format!("{:x}", Sha256::digest(METADATA.as_bytes()))).execute(&pool).await.unwrap();
    sqlx::query("UPDATE recordings SET metadata_observation_id=? WHERE id=?")
        .bind(OBSERVATION)
        .bind(PROVIDER_RECORDING)
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query(
        "UPDATE provider_source_state SET generation=3,observation_id=? WHERE recording_id=?",
    )
    .bind(OBSERVATION)
    .bind(PROVIDER_RECORDING)
    .execute(&pool)
    .await
    .unwrap();
    sqlx::query("INSERT INTO source_metadata(source_id,release_id,title,artist,duration_ms,disc_number,track_number,observation_id,provenance) VALUES (?,?,'Provider track','Provider performer',123000,1,2,?,'provider_response')")
        .bind(PROVIDER_SOURCE).bind(PROVIDER_RELEASE).bind(OBSERVATION).execute(&pool).await.unwrap();
    sqlx::query("INSERT INTO account_source_credits(user_id,source_id,recording_id,release_id,artist_id) VALUES ('legacy-admin',?,?,?,'private-provider-performer')")
        .bind(PROVIDER_SOURCE).bind(PROVIDER_RECORDING).bind(PROVIDER_RELEASE).execute(&pool).await.unwrap();
    through(&pool, 44).await;

    let origin = json!({
        "installation_id": INSTALLATION, "installation_revision": 1,
        "service_id": "release-credit-service", "service_revision": 1,
        "provider": "org.hito.content-fixture", "provider_playlist_id": "playlist/known",
        "snapshot": "known-one", "attribution": {"label": "Owned provider"}, "imported_at": 123456
    });
    let entries = [("30000000-0000-4000-8000-000000000008", "first"),
        ("30000000-0000-4000-8000-000000000009", "again")]
        .into_iter().enumerate().map(|(position, (entry_id, occurrence_id))| json!({
            "entry_id":entry_id,"recording_id":PROVIDER_RECORDING,"position":position,
            "origin":{"kind":"recording","provider_recording_id":"recording/known","occurrence_id":occurrence_id},
            "title":null,"artist":null
        })).collect::<Vec<_>>();
    let publication = json!({"kind":"playlist_imported","publication_id":PUBLICATION,"snapshot":{
        "playlist_id":IMPORTED_PLAYLIST,"name":"Provider copy","origin":origin,
        "recordings":[{"recording_id":PROVIDER_RECORDING,"library_id":"30000000-0000-4000-8000-000000000002",
            "release_id":PROVIDER_RELEASE,"observation_id":OBSERVATION,"metadata":serde_json::from_str::<serde_json::Value>(METADATA).unwrap()}],
        "entries":entries
    }});
    sqlx::query("INSERT INTO sync_events(sequence,user_id,origin,event_id,payload) VALUES (77,'legacy-admin','playlist_import',?,?)")
        .bind(PUBLICATION).bind(serde_json::to_string_pretty(&publication).unwrap()).execute(&pool).await.unwrap();
    let request = request();
    let digest = Sha256::digest(
        serde_json::to_vec(&("hito-playlist-import:v1", INSTALLATION, &request)).unwrap(),
    );
    sqlx::query("INSERT INTO playlist_import_receipts(rowid,user_id,request_id,request_digest,playlist_id,publication_sequence) VALUES (811,'legacy-admin',?,?,?,77)")
        .bind(request.request_id.to_string()).bind(format!("{digest:x}")).bind(IMPORTED_PLAYLIST).execute(&pool).await.unwrap();
    sqlx::query("INSERT INTO metadata_observations(sequence,id,recording_id,provider,origin,title,artist,duration_ms) VALUES (2000000,'deleted-observation',?,'fixture','provider_response','Deleted','Deleted',0)")
        .bind(PROVIDER_RECORDING).execute(&pool).await.unwrap();
    sqlx::query("DELETE FROM metadata_observations WHERE id='deleted-observation'")
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO sync_events(sequence,user_id,origin,device_id,event_id,payload) VALUES (1900,'legacy-admin','client_operation','fixture','deleted-event','{}')").execute(&pool).await.unwrap();
    sqlx::query("DELETE FROM sync_events WHERE event_id='deleted-event'")
        .execute(&pool)
        .await
        .unwrap();
    pool.close().await;
    fixture
}
