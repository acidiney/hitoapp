#[path = "legacy_library.rs"]
mod legacy;
pub use legacy::{LIBRARY, Legacy, PLAYLIST, RECORDING, SOURCE};

use sqlx::{SqlitePool, migrate::Migrator, sqlite::SqliteConnectOptions};
use std::borrow::Cow;

pub const FIRST: &str = "20000000-0000-4000-8000-000000000001";
pub const LAST: &str = "20000000-0000-4000-8000-0000000186a0";
pub const INSTALLATION: &str = "scale-installation";
pub const COUNT: i64 = 100_000;
pub const RECORDING_COLUMNS: &str = "rowid,id,library_id,release_id,title,artist,duration_ms,initial_file_checksum,provenance,metadata_source_id,metadata_observation_id";
pub const OBSERVATION_COLUMNS: &str = "sequence,id,source_id,recording_id,release_id,provider,origin,observed_at,file_checksum,inspection_version,title,artist,album,album_artist,duration_ms,disc_number,track_number,tags,plugin_installation_id,provider_recording_id,provider_revision,provider_metadata,provider_digest";
pub const PROVIDER_SOURCE_COLUMNS: &str =
    "source_id,installation_id,recording_id,provider_source_id,label,generation";

pub async fn connect(fixture: &Legacy) -> SqlitePool {
    SqlitePool::connect_with(
        SqliteConnectOptions::new()
            .filename(fixture.directory.path().join("hito.db"))
            .foreign_keys(true),
    )
    .await
    .unwrap()
}

pub async fn seed(fixture: &Legacy, provider_url: &str) -> i64 {
    let pool = connect(fixture).await;
    let all = sqlx::migrate!("./migrations");
    Migrator {
        migrations: Cow::Owned(
            all.iter()
                .filter(|migration| migration.version <= 25)
                .cloned()
                .collect(),
        ),
        ..Migrator::DEFAULT
    }
    .run(&pool)
    .await
    .unwrap();
    let mut tx = pool.begin().await.unwrap();
    sqlx::query("INSERT INTO plugin_services(id,name,base_url) VALUES ('scale-service','Owned upgrade provider',?)")
        .bind(provider_url).execute(&mut *tx).await.unwrap();
    sqlx::raw_sql(
        "INSERT INTO plugin_installations(id,user_id,service_id,manifest)
            VALUES ('scale-installation','legacy-admin','scale-service',
            '{\"id\":\"org.hito.content-fixture\",\"name\":\"Owned upgrade provider\",\"protocol_version\":1,\"capabilities\":[\"sources\"]}');
        INSERT INTO libraries(id,name,owner_id) VALUES ('scale-library','Provider scale library','legacy-admin');
        INSERT INTO library_members(library_id,user_id) VALUES ('scale-library','legacy-admin');
        INSERT INTO releases(id,library_id,title,artist,identity_key)
            VALUES ('scale-release','scale-library','Scale album','Scale artist','scale-album');
        WITH RECURSIVE n(i) AS (VALUES(1) UNION ALL SELECT i + 1 FROM n WHERE i < 100000)
        INSERT INTO recordings(rowid,id,library_id,release_id,title,artist,duration_ms,provenance)
            SELECT i * 2 + 1000,printf('20000000-0000-4000-8000-%012x',i),'scale-library',
            'scale-release','Scale track ' || i,'Scale artist',1000 + i,'provider_response' FROM n;
        INSERT INTO provider_items(installation_id,provider_recording_id,recording_id)
            SELECT 'scale-installation','recording/' || ((rowid - 1000) / 2),id
            FROM recordings WHERE library_id = 'scale-library';
        INSERT INTO metadata_observations(id,recording_id,release_id,provider,origin,observed_at,
            title,artist,album,album_artist,duration_ms,plugin_installation_id,provider_recording_id,
            provider_revision,provider_metadata,provider_digest)
            SELECT 'observed-' || r.id,r.id,r.release_id,'org.hito.content-fixture','provider_response',123456,
            r.title,r.artist,'Scale album','Scale artist',r.duration_ms,item.installation_id,item.provider_recording_id,
            'before',json_object('id',item.provider_recording_id,'title',r.title,'artist',r.artist),'digest-' || r.id
            FROM recordings r JOIN provider_items item ON item.recording_id = r.id
            WHERE r.library_id = 'scale-library';
        UPDATE recordings SET metadata_observation_id = 'observed-' || id WHERE library_id = 'scale-library';
        UPDATE provider_source_state SET generation = 3,observation_id = 'observed-' || recording_id
            WHERE recording_id IN (SELECT id FROM recordings WHERE library_id = 'scale-library' AND rowid % 4 = 2);
        INSERT INTO sources(id,recording_id,provider,provider_id,ready)
            SELECT 'current-' || r.id,r.id,'plugin','audio-' || r.id,1 FROM recordings r
            WHERE r.library_id = 'scale-library' AND r.rowid <= 1128 AND r.rowid % 4 = 2;
        INSERT INTO sources(id,recording_id,provider,provider_id,ready)
            SELECT 'historical-' || r.id,r.id,'plugin','old-audio-' || r.id,0 FROM recordings r
            WHERE r.library_id = 'scale-library' AND r.rowid <= 1128 AND r.rowid % 4 = 2;
        INSERT INTO provider_sources(source_id,installation_id,recording_id,provider_source_id,label,generation)
            SELECT s.id,'scale-installation',s.recording_id,s.provider_id,'Owned source',CASE s.ready WHEN 1 THEN 3 ELSE 2 END
            FROM sources s JOIN recordings r ON r.id = s.recording_id WHERE r.library_id = 'scale-library';
        INSERT INTO source_metadata(source_id,release_id,title,artist,duration_ms,observation_id,provenance)
            SELECT s.id,r.release_id,r.title,r.artist,r.duration_ms,r.metadata_observation_id,'provider_response'
            FROM sources s JOIN recordings r ON r.id = s.recording_id WHERE r.library_id = 'scale-library';
        INSERT INTO personal_recording_metadata(user_id,recording_id,override_title,override_artist,title,artist,title_operation_id,artist_operation_id)
            VALUES ('legacy-admin','20000000-0000-4000-8000-000000000001','My title','My artist','My title','My artist','title-operation','artist-operation');
        INSERT INTO account_artists(user_id,id,library_id,name) VALUES
            ('legacy-admin','scale-artist','scale-library','Scale artist'),
            ('legacy-admin','personal-artist','scale-library','My artist');
        INSERT INTO account_releases(user_id,id,library_id,title,artist,artist_id)
            VALUES ('legacy-admin','scale-release','scale-library','Scale album','Scale artist','scale-artist');
        INSERT INTO account_recording_credits(user_id,recording_id,artist_id)
            SELECT 'legacy-admin',r.id,CASE WHEN p.recording_id IS NULL THEN 'scale-artist' ELSE 'personal-artist' END
            FROM recordings r LEFT JOIN personal_recording_metadata p ON p.recording_id = r.id
            WHERE r.library_id = 'scale-library';
        INSERT INTO account_source_credits(user_id,source_id,recording_id,release_id,artist_id)
            SELECT 'legacy-admin',s.id,r.id,r.release_id,'scale-artist'
            FROM sources s JOIN recordings r ON r.id = s.recording_id WHERE r.library_id = 'scale-library';
        INSERT INTO likes(user_id,recording_id,liked)
            SELECT 'legacy-admin',id,1 FROM recordings WHERE library_id = 'scale-library';
        INSERT INTO listening_progress(user_id,recording_id,position_ms,speed,updated_at)
            SELECT 'legacy-admin',id,42,1.25,123456 FROM recordings WHERE library_id = 'scale-library';
        INSERT INTO playlist_entries(playlist_id,id,recording_id,position)
            SELECT '10000000-0000-4000-8000-000000000004','entry-' || id,id,rowid
            FROM recordings WHERE library_id = 'scale-library';
        INSERT INTO playlist_entries(playlist_id,id,recording_id,position)
            VALUES ('10000000-0000-4000-8000-000000000004','duplicate-first','20000000-0000-4000-8000-000000000001',300000);
        INSERT INTO metadata_observations(sequence,id,recording_id,provider,origin,title,artist,duration_ms)
            VALUES (2000000,'deleted-high-water','20000000-0000-4000-8000-000000000001','fixture','provider_response','Deleted observation','Scale artist',1000);
        DELETE FROM metadata_observations WHERE id = 'deleted-high-water';",
    )
    .execute(&mut *tx)
    .await
    .unwrap();
    for (table, columns) in [
        ("recordings", RECORDING_COLUMNS),
        ("metadata_observations", OBSERVATION_COLUMNS),
        ("provider_sources", PROVIDER_SOURCE_COLUMNS),
        ("provider_source_state", "*"),
        ("sources", "*"),
        ("source_metadata", "*"),
        ("releases", "*"),
        ("personal_recording_metadata", "*"),
        ("likes", "*"),
        ("listening_progress", "*"),
        ("playlists", "*"),
        ("playlist_entries", "*"),
    ] {
        sqlx::query(&format!(
            "CREATE TABLE before_{table} AS SELECT {columns} FROM {table}"
        ))
        .execute(&mut *tx)
        .await
        .unwrap();
    }
    tx.commit().await.unwrap();
    let high_water =
        sqlx::query_scalar("SELECT seq FROM sqlite_sequence WHERE name = 'metadata_observations'")
            .fetch_one(&pool)
            .await
            .unwrap();
    pool.close().await;
    high_water
}

pub async fn assert_unchanged(pool: &SqlitePool, table: &str, columns: &str) {
    let differences: i64 = sqlx::query_scalar(&format!(
        "SELECT (SELECT count(*) FROM (SELECT {columns} FROM {table} EXCEPT SELECT * FROM before_{table})) +
            (SELECT count(*) FROM (SELECT * FROM before_{table} EXCEPT SELECT {columns} FROM {table}))"
    )).fetch_one(pool).await.unwrap();
    assert_eq!(differences, 0, "The upgrade changed existing {table}");
}
