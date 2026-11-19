#[path = "fixtures/source_refresh.rs"]
mod fixture;
use hito_core::test_support::support;

use fixture::Fixture;
use hito_core::{Action, Operation, ResolveSource, SourceReadinessPreference, SyncBatch};
use serde_json::json;
use std::future::pending;
use uuid::Uuid;

async fn offer_a_candidate(fixture: &Fixture, title: &str) {
    fixture
        .server
        .reply(
            "v1/search",
            json!({"items": [{"id": "owned/audio-candidate", "title": title, "artist": "Accepted artist", "duration_ms": 1000}], "next_cursor": null}),
        )
        .await;
    fixture
        .server
        .reply(
            "v1/recordings/detail",
            json!({"id": "owned/audio-candidate", "title": title, "artist": "Accepted artist", "duration_ms": 1000}),
        )
        .await;
    fixture
        .server
        .reply(
            "v1/sources",
            json!({"items": [{"id": "owned/audio-source", "label": "Owned automatic source", "ready": true}]}),
        )
        .await;
}

async fn ready(fixture: &Fixture) -> bool {
    fixture
        .hito
        .catalog
        .sources(&fixture.actor, &fixture.recording.id)
        .await
        .unwrap()
        .iter()
        .any(|source| source.ready)
}

async fn pool(fixture: &Fixture) -> sqlx::SqlitePool {
    sqlx::SqlitePool::connect_with(
        sqlx::sqlite::SqliteConnectOptions::new()
            .filename(fixture.directory.path().join("hito.db"))
            .foreign_keys(true),
    )
    .await
    .unwrap()
}

async fn time_passes(fixture: &Fixture) {
    sqlx::query("UPDATE source_readiness_jobs SET next_run_at = unixepoch()")
        .execute(&pool(fixture).await)
        .await
        .unwrap();
}

async fn waiting_seconds(fixture: &Fixture) -> i64 {
    sqlx::query_scalar("SELECT next_run_at - unixepoch() FROM source_readiness_jobs")
        .fetch_one(&pool(fixture).await)
        .await
        .unwrap()
}

#[tokio::test]
async fn an_account_on_a_running_server_is_walked_once_it_has_settled() {
    let fixture = Fixture::start(json!({"items": []})).await;
    offer_a_candidate(&fixture, "Accepted title").await;
    assert!(
        !fixture
            .hito
            .catalog
            .run_next_source_readiness_until(pending())
            .await
            .unwrap(),
        "a newly seen account does not start searching at once"
    );
    assert!(
        waiting_seconds(&fixture).await >= 100,
        "the first walk waits for the account to settle"
    );
    assert!(!ready(&fixture).await);
    time_passes(&fixture).await;
    assert!(
        fixture
            .hito
            .catalog
            .run_next_source_readiness_until(pending())
            .await
            .unwrap()
    );
    assert!(
        ready(&fixture).await,
        "an account that joined after the server started is still walked"
    );
}

#[tokio::test]
async fn the_background_walk_prepares_a_confident_source_and_then_paces_itself() {
    let fixture = Fixture::start(json!({"items": []})).await;
    assert!(!ready(&fixture).await);
    offer_a_candidate(&fixture, "Accepted title").await;
    fixture
        .hito
        .catalog
        .run_next_source_readiness_until(pending())
        .await
        .unwrap();
    time_passes(&fixture).await;
    assert!(
        fixture
            .hito
            .catalog
            .run_next_source_readiness_until(pending())
            .await
            .unwrap()
    );
    assert!(
        ready(&fixture).await,
        "a confident match is applied in the background"
    );
    assert!(
        waiting_seconds(&fixture).await >= 100,
        "the next provider call waits at least two minutes"
    );
    assert!(
        !fixture
            .hito
            .catalog
            .run_next_source_readiness_until(pending())
            .await
            .unwrap(),
        "nothing is due until the pace allows"
    );
}

#[tokio::test]
async fn a_guess_by_provider_rank_is_never_applied_in_the_background() {
    let fixture = Fixture::start(json!({"items": []})).await;
    offer_a_candidate(&fixture, "A different song entirely").await;
    fixture
        .hito
        .catalog
        .run_next_source_readiness_until(pending())
        .await
        .unwrap();
    time_passes(&fixture).await;
    fixture
        .hito
        .catalog
        .run_next_source_readiness_until(pending())
        .await
        .unwrap();
    assert!(
        !ready(&fixture).await,
        "an unmatched candidate stays for the listener to decide"
    );
    fixture
        .hito
        .catalog
        .resolve_source_until(
            &fixture.actor,
            &fixture.recording.id,
            ResolveSource {
                request_id: Uuid::new_v4(),
            },
            pending(),
        )
        .await
        .unwrap();
    assert!(
        ready(&fixture).await,
        "the listener's own playback may still use the provider's best guess"
    );
}

#[tokio::test]
async fn a_listener_who_turned_it_off_or_is_listening_is_left_alone() {
    let fixture = Fixture::start(json!({"items": []})).await;
    offer_a_candidate(&fixture, "Accepted title").await;
    assert_eq!(
        fixture
            .hito
            .catalog
            .source_readiness_preference(&fixture.actor)
            .await
            .unwrap(),
        SourceReadinessPreference { enabled: true }
    );
    fixture
        .hito
        .catalog
        .set_source_readiness_preference(
            &fixture.actor,
            SourceReadinessPreference { enabled: false },
        )
        .await
        .unwrap();
    fixture
        .hito
        .catalog
        .run_next_source_readiness_until(pending())
        .await
        .unwrap();
    time_passes(&fixture).await;
    fixture
        .hito
        .catalog
        .run_next_source_readiness_until(pending())
        .await
        .unwrap();
    assert!(
        !ready(&fixture).await,
        "a switched-off account is not searched"
    );

    fixture
        .hito
        .collection
        .apply(
            &fixture.actor,
            SyncBatch {
                device_id: "readiness-listener".into(),
                operations: vec![Operation {
                    id: Uuid::new_v4(),
                    action: Action::ProgressSet {
                        recording_id: Uuid::parse_str(&fixture.recording.id).unwrap(),
                        position_ms: 1000,
                        speed: 1.0,
                    },
                }],
            },
        )
        .await
        .unwrap();
    fixture
        .hito
        .catalog
        .set_source_readiness_preference(
            &fixture.actor,
            SourceReadinessPreference { enabled: true },
        )
        .await
        .unwrap();
    fixture
        .hito
        .catalog
        .run_next_source_readiness_until(pending())
        .await
        .unwrap();
    assert!(
        !ready(&fixture).await,
        "it waits while the account is listening"
    );
}

#[tokio::test]
async fn a_held_host_prepares_nothing_until_it_is_released() {
    let fixture = Fixture::start(json!({"items": []})).await;
    offer_a_candidate(&fixture, "Accepted title").await;
    fixture
        .hito
        .catalog
        .run_next_source_readiness_until(pending())
        .await
        .unwrap();
    time_passes(&fixture).await;
    fixture.hito.catalog.hold_background_preparation(true);
    assert!(
        !fixture
            .hito
            .catalog
            .run_next_source_readiness_until(pending())
            .await
            .unwrap(),
        "a held host claims no walk"
    );
    assert!(!ready(&fixture).await);
    fixture.hito.catalog.hold_background_preparation(false);
    assert!(
        fixture
            .hito
            .catalog
            .run_next_source_readiness_until(pending())
            .await
            .unwrap(),
        "the walk resumes once the host is released"
    );
    assert!(ready(&fixture).await);
}
