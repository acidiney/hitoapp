#[path = "fixtures/provider_bindings_upgrade.rs"]
mod fixture;
use hito_core::test_support::content_service as provider;
use hito_core::test_support::support;

use hito_core::{AdoptProviderRecording, Search};
use serde_json::json;
use sqlx::{Connection, SqliteConnection, sqlite::SqliteConnectOptions};
use std::sync::{
    Arc,
    atomic::{AtomicU64, Ordering},
};

const STEPS_PER_BINDING: u64 = 2_000;

#[tokio::test]
async fn populated_binding_upgrade_preserves_one_hundred_thousand_provider_recordings() {
    let legacy = fixture::Legacy::seed().await;
    let server = provider::ProviderServer::start().await;
    let high_water = fixture::seed(&legacy, &server.url).await;
    assert_eq!(high_water, 2_000_000);

    let budget = fixture::COUNT as u64 * STEPS_PER_BINDING;
    let work = upgrade_work(&legacy, budget).await;
    eprintln!("Populated 100000-provider-binding upgrade: steps~{work} budget={budget}");
    assert!(
        work < budget,
        "Provider binding upgrade exceeded its work budget of {STEPS_PER_BINDING} steps per binding"
    );
    let (hito, actor) = legacy.open().await;
    let page = hito
        .catalog
        .recordings(
            &actor,
            Search {
                library: Some("scale-library".into()),
                liked: Some(true),
                limit: Some(1),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    assert_eq!(page.total, fixture::COUNT);
    let metadata = hito
        .catalog
        .recording_metadata(&actor, fixture::FIRST, Default::default())
        .await
        .unwrap();
    assert_eq!(metadata.title.value, "My title");
    assert_eq!(metadata.artist.value, "My artist");
    assert_eq!(
        metadata.title.operation_id.as_deref(),
        Some("title-operation")
    );
    assert_eq!(
        metadata.artist.operation_id.as_deref(),
        Some("artist-operation")
    );
    assert_eq!(
        metadata.selected_observation.unwrap().id,
        format!("observed-{}", fixture::FIRST)
    );
    let sources = hito.catalog.sources(&actor, fixture::FIRST).await.unwrap();
    assert_eq!(sources.len(), 1);
    assert_eq!(sources[0].id, format!("current-{}", fixture::FIRST));
    assert!(sources[0].ready);
    assert!(
        hito.catalog
            .sources(&actor, fixture::LAST)
            .await
            .unwrap()
            .is_empty()
    );
    assert_eq!(
        support::audio_bytes(
            hito.playback
                .audio_file(&actor, fixture::SOURCE)
                .await
                .unwrap()
        ),
        std::fs::read(&legacy.original).unwrap()
    );
    assert_eq!(
        hito.collection
            .playlist(&actor, fixture::PLAYLIST)
            .await
            .unwrap()
            .entries[0]
            .recording_id
            .as_deref(),
        Some(fixture::RECORDING)
    );
    assert!(
        hito.catalog
            .libraries(&actor)
            .await
            .unwrap()
            .iter()
            .any(|library| library.id == fixture::LIBRARY)
    );

    let pool = fixture::connect(&legacy).await;
    for (table, columns) in [
        ("recordings", fixture::RECORDING_COLUMNS),
        ("metadata_observations", fixture::OBSERVATION_COLUMNS),
        ("provider_sources", fixture::PROVIDER_SOURCE_COLUMNS),
        ("sources", "*"),
        ("source_metadata", "*"),
        ("releases", "*"),
        ("personal_recording_metadata", "*"),
        ("likes", "*"),
        ("listening_progress", "*"),
        ("playlists", "*"),
        ("playlist_entries", "*"),
    ] {
        fixture::assert_unchanged(&pool, table, columns).await;
    }
    let retained: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM provider_source_state state JOIN provider_items item ON item.id = state.binding_id
        JOIN before_provider_source_state prior ON prior.recording_id = item.recording_id
            AND prior.generation = state.generation AND prior.observation_id = state.observation_id"
    ).fetch_one(&pool).await.unwrap();
    assert_eq!(retained, fixture::COUNT / 2);
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM provider_source_state")
            .fetch_one(&pool)
            .await
            .unwrap(),
        retained
    );
    let selected: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM recordings r JOIN provider_items item ON item.id = r.metadata_binding_id
        JOIN metadata_observations observation ON observation.id = r.metadata_observation_id AND observation.provider_binding_id = item.id
        WHERE r.library_id = 'scale-library' AND item.recording_id = r.id AND item.role = 'primary' AND item.retired = 0"
    ).fetch_one(&pool).await.unwrap();
    assert_eq!(selected, fixture::COUNT);
    assert_eq!(
        sqlx::query_scalar::<_, i64>(
            "SELECT seq FROM sqlite_sequence WHERE name = 'metadata_observations'"
        )
        .fetch_one(&pool)
        .await
        .unwrap(),
        high_water
    );
    assert!(
        sqlx::query("PRAGMA foreign_key_check")
            .fetch_all(&pool)
            .await
            .unwrap()
            .is_empty()
    );
    let integrity: String = sqlx::query_scalar("PRAGMA integrity_check")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(integrity, "ok");
    sqlx::query("INSERT INTO recordings_fts(recordings_fts,rank) VALUES ('integrity-check',1)")
        .execute(&pool)
        .await
        .unwrap();

    server.reply("v1/recordings/detail", json!({
        "id": "recording/1", "title": "Refreshed title", "artist": "Scale artist", "revision": "after"
    })).await;
    server.reply("v1/sources", json!({"items": []})).await;
    let refreshed = hito
        .catalog
        .adopt_provider(
            &actor,
            fixture::INSTALLATION,
            AdoptProviderRecording {
                installation_revision: 1,
                provider_recording_id: "recording/1".into(),
            },
        )
        .await
        .unwrap();
    assert_eq!(refreshed.id, fixture::FIRST);
    let observed = hito
        .catalog
        .recording_metadata(&actor, fixture::FIRST, Default::default())
        .await
        .unwrap()
        .selected_observation
        .unwrap();
    let next_sequence: i64 =
        sqlx::query_scalar("SELECT sequence FROM metadata_observations WHERE id = ?")
            .bind(observed.id)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert!(
        next_sequence > high_water,
        "New observations must not reuse retired sequence values"
    );
    pool.close().await;
}

async fn upgrade_work(legacy: &fixture::Legacy, budget: u64) -> u64 {
    let options = SqliteConnectOptions::new()
        .filename(legacy.directory.path().join("hito.db"))
        .foreign_keys(true);
    let mut connection = SqliteConnection::connect_with(&options).await.unwrap();
    let steps = Arc::new(AtomicU64::new(0));
    let progress = Arc::clone(&steps);
    connection
        .lock_handle()
        .await
        .unwrap()
        .set_progress_handler(1000, move || {
            progress.fetch_add(1000, Ordering::Relaxed) + 1000 < budget
        });
    let result = sqlx::migrate!("./migrations").run(&mut connection).await;
    connection
        .lock_handle()
        .await
        .unwrap()
        .remove_progress_handler();
    connection.close().await.unwrap();
    assert!(
        result.is_ok(),
        "Provider binding upgrade exceeded its work budget or failed: {result:?}"
    );
    steps.load(Ordering::Relaxed)
}
