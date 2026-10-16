#[path = "fixtures/legacy_library.rs"]
mod legacy;
#[path = "fixtures/content_service.rs"]
mod provider;
mod support;

use serde_json::json;
use sqlx::{migrate::Migrator, sqlite::SqliteConnectOptions};
use std::{
    borrow::Cow,
    time::{Duration, Instant},
};

#[tokio::test]
async fn upgrade_preserves_source_identity_and_keeps_historical_bindings_out_of_playback() {
    let fixture = legacy::Legacy::seed().await;
    let server = provider::ProviderServer::start().await;
    let path = fixture.directory.path().join("hito.db");
    let pool = sqlx::SqlitePool::connect_with(
        SqliteConnectOptions::new()
            .filename(&path)
            .foreign_keys(true),
    )
    .await
    .unwrap();
    let all = sqlx::migrate!("./migrations");
    Migrator {
        migrations: Cow::Owned(all.iter().filter(|m| m.version <= 21).cloned().collect()),
        ..Migrator::DEFAULT
    }
    .run(&pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO plugin_services(id,name,base_url) VALUES ('service','Upgrade provider',?)",
    )
    .bind(&server.url)
    .execute(&pool)
    .await
    .unwrap();
    sqlx::query("INSERT INTO plugin_installations(id,user_id,service_id,manifest) VALUES ('installation','legacy-admin','service',?)")
        .bind(json!({"id":"org.hito.content-fixture","name":"Upgrade provider","protocol_version":1,"capabilities":["sources"]}).to_string()).execute(&pool).await.unwrap();
    sqlx::raw_sql("INSERT INTO libraries(id,name,owner_id) VALUES ('personal','Provider library','legacy-admin');
        INSERT INTO library_members VALUES ('personal','legacy-admin');
        INSERT INTO recordings(id,library_id,title,artist,duration_ms,provenance) VALUES ('saved','personal','Accepted title','Accepted artist',1000,'provider_response');
        INSERT INTO provider_items VALUES ('installation','prior/item','saved');
        INSERT INTO metadata_observations(id,recording_id,provider,origin,title,artist,duration_ms,plugin_installation_id,provider_recording_id)
            VALUES ('accepted','saved','org.hito.content-fixture','provider_response','Accepted title','Accepted artist',1000,'installation','prior/item');
        UPDATE recordings SET metadata_observation_id='accepted' WHERE id='saved';
        INSERT INTO personal_recording_metadata(user_id,recording_id,override_title,title,artist,title_operation_id)
            VALUES ('legacy-admin','saved','Personal title','Personal title','Accepted artist','private-title');
        INSERT INTO likes VALUES ('legacy-admin','saved',1);
        INSERT INTO listening_progress(user_id,recording_id,position_ms,speed) VALUES ('legacy-admin','saved',500,1.25);
        WITH RECURSIVE n(i) AS (VALUES(1) UNION ALL SELECT i+1 FROM n WHERE i<100016)
            INSERT INTO sources(id,recording_id,provider,provider_id,ready) SELECT 'old-'||i,'saved','plugin','audio-'||i,i>100000 FROM n;
        INSERT INTO provider_sources SELECT id,'installation',recording_id,provider_id,'Original' FROM sources WHERE recording_id='saved';
        INSERT INTO source_metadata(source_id,title,artist,duration_ms,observation_id,provenance)
            SELECT id,'Accepted title','Accepted artist',1000,'accepted','provider_response' FROM sources WHERE recording_id='saved';
        ANALYZE;")
        .execute(&pool).await.unwrap();
    let before: String = sqlx::query_scalar("SELECT json_group_array(json_array(rowid,id,title,artist,metadata_observation_id)) FROM recordings ORDER BY id").fetch_one(&pool).await.unwrap();
    pool.close().await;

    let started = Instant::now();
    let (hito, actor) = fixture.open().await;
    let cached = hito.catalog.sources(&actor, "saved").await.unwrap();
    assert_eq!(
        cached.len(),
        16,
        "Withdrawn source history must not become the current snapshot"
    );
    assert!(cached.iter().all(|source| source.ready));
    server
        .reply(
            "v1/sources",
            json!({"items":[
                {"id":"audio-100001","label":"Still ready","ready":true},
                {"id":"new-audio","label":"New audio","ready":true}
            ]}),
        )
        .await;
    let refreshed = hito.catalog.refresh_sources(&actor, "saved").await.unwrap();
    assert_eq!(refreshed.len(), 2);
    assert!(refreshed.iter().all(|source| source.ready));
    assert!(refreshed.iter().any(|source| source.id == "old-100001"));
    let details = hito
        .catalog
        .recording_metadata(&actor, "saved", Default::default())
        .await
        .unwrap();
    assert_eq!(details.title.value, "Personal title");
    assert_eq!(details.title.inherited, "Accepted title");
    assert_eq!(details.observations.items.len(), 1);
    assert_eq!(details.selected_observation.unwrap().id, "accepted");
    assert!(server.requests("v1/recordings/detail").await.is_empty());
    assert_eq!(
        server.requests("v1/sources").await[0].1,
        json!({"recording_id":"prior/item"})
    );
    assert_eq!(
        support::audio_bytes(
            hito.catalog
                .audio_file(&actor, legacy::SOURCE)
                .await
                .unwrap()
        ),
        std::fs::read(&fixture.original).unwrap()
    );
    assert_eq!(
        hito.collection
            .playlist(&actor, legacy::PLAYLIST)
            .await
            .unwrap()
            .entries[0]
            .recording_id,
        Some(legacy::RECORDING.to_string())
    );
    assert!(
        started.elapsed() < Duration::from_secs(15),
        "Upgrade and first playback refresh exceeded their development smoke-test deadline"
    );
    let pool = sqlx::SqlitePool::connect_with(
        SqliteConnectOptions::new()
            .filename(path)
            .foreign_keys(true),
    )
    .await
    .unwrap();
    let after: String = sqlx::query_scalar("SELECT json_group_array(json_array(rowid,id,title,artist,metadata_observation_id)) FROM recordings ORDER BY id").fetch_one(&pool).await.unwrap();
    assert_eq!(after, before);
    assert_eq!(
        sqlx::query_scalar::<_, i64>(
            "SELECT count(*) FROM provider_sources WHERE recording_id='saved'"
        )
        .fetch_one(&pool)
        .await
        .unwrap(),
        100017
    );
    assert_eq!(
        sqlx::query_scalar::<_, i64>(
            "SELECT count(*) FROM source_metadata WHERE observation_id='accepted'"
        )
        .fetch_one(&pool)
        .await
        .unwrap(),
        100017
    );
    assert_eq!(sqlx::query_scalar::<_,i64>("SELECT count(*) FROM listening_progress WHERE user_id='legacy-admin' AND recording_id='saved' AND position_ms=500 AND speed=1.25").fetch_one(&pool).await.unwrap(),1);
    assert!(
        sqlx::query("PRAGMA foreign_key_check")
            .fetch_all(&pool)
            .await
            .unwrap()
            .is_empty()
    );
    sqlx::query("INSERT INTO recordings_fts(recordings_fts,rank) VALUES ('integrity-check',1)")
        .execute(&pool)
        .await
        .unwrap();
    pool.close().await;
}
