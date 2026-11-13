use super::{Reviewed, prepare, private};
use crate::{
    artist_review_fixture as review, catalog_album_header_fixture as catalog,
    provider_fixture::ProviderServer, support,
};
use axum::{Router, http::StatusCode};
use futures_util::FutureExt;
use hito_core::{
    CoreConfig, HostDeviceId,
    test_support::{database_owner::DatabaseOwner, hang_guard::hang_guard},
};
use hito_plugin_sdk::CatalogEditionTrackList;
use hito_testkit::fake_plugin::{FakePlugin, Reply};
use serde_json::{Value, json};
use std::{
    panic::{AssertUnwindSafe, resume_unwind},
    sync::Arc,
    time::Duration,
};

#[tokio::test]
async fn selected_edition_counts_audio_occurrences_and_keeps_unknown_total_duration() {
    let response = edition_stats_response(catalog::edition()).await;
    assert_eq!(
        response.get("stats"),
        Some(&json!({"audio_tracks": 4, "total_duration_ms": null})),
        "Core must count repeated audio occurrences, exclude data and pregap, and preserve unknown duration"
    );
}

#[tokio::test]
async fn selected_edition_sums_known_audio_occurrences_across_media() {
    let mut edition = catalog::edition();
    edition.media[0].tracks[2].duration_ms = Some(90_000);
    let response = edition_stats_response(edition).await;
    assert_eq!(
        response.get("stats"),
        Some(&json!({"audio_tracks": 4, "total_duration_ms": 375_000})),
        "Core must sum every audio occurrence across media while excluding nonzero data and pregap durations"
    );
}

async fn edition_stats_response(expected_edition: CatalogEditionTrackList) -> Value {
    let directory = Arc::new(tempfile::tempdir().unwrap());
    let music = directory.path().join("music");
    std::fs::create_dir(&music).unwrap();
    let audio = music.join("owned-edition-stats-original.wav");
    support::write_wav(&audio);
    let original = std::fs::read(&audio).unwrap();
    let owner = DatabaseOwner::retain(directory.clone());
    let mut config = CoreConfig::new(
        directory.path().join("hito.db"),
        music,
        HostDeviceId::new("edition-stats-http").unwrap(),
    );
    owner.attach(&mut config);
    let services = hang_guard(
        "edition stats owned services",
        Box::pin(hito_core::open(config)),
    )
    .await
    .unwrap();
    let app = hito_server::router(services.clone());
    let origin = hang_guard(
        "edition stats owned origin",
        Box::pin(ProviderServer::start()),
    )
    .await;
    let mut provider = hang_guard("edition stats owned catalog", Box::pin(catalog::start())).await;
    provider.reply(catalog::TRACK_LIST, Reply::Json(json!(expected_edition)));
    let outcome = AssertUnwindSafe(Box::pin(async {
        let reviewed = hang_guard(
            "edition stats reviewed association",
            Box::pin(prepare(&app, &origin, &provider)),
        )
        .await;
        tokio::time::timeout(
            Duration::from_secs(30),
            read_selected_edition(&app, &reviewed, &origin, &provider, &expected_edition),
        )
        .await
        .expect("The edition statistics workflow must finish within its deadline")
    }))
    .catch_unwind()
    .await;
    let joined = provider.finish().await;
    origin.shutdown().await;
    let playback_closed = services.playback.close().await;
    let replicas_closed = services.replicas.close().await;
    let replication_closed = services.catalog_replication.close().await;
    drop(app);
    drop(services);
    let retired = owner.retire(Duration::from_secs(5)).await;
    assert!(joined);
    assert!(playback_closed.is_ok() && replicas_closed.is_ok() && replication_closed.is_ok());
    assert!(retired.resources_retired && !retired.deadline_exceeded);
    assert_eq!(owner.observe().active_resources(), 0);
    assert_eq!(std::fs::read(audio).unwrap(), original);
    match outcome {
        Ok(response) => response,
        Err(panic) => resume_unwind(panic),
    }
}

async fn read_selected_edition(
    app: &Router,
    reviewed: &Reviewed,
    origin: &ProviderServer,
    provider: &FakePlugin,
    expected_edition: &CatalogEditionTrackList,
) -> Value {
    let Reviewed {
        account,
        peer,
        imported,
        association,
    } = reviewed;
    let before = imported.snapshot(app, account).await;
    let origin_before = review::origin_counts(origin).await;
    let peer_before = peer.get(app, "/api/v1/recordings").await;
    let peer_plugins = peer.get(app, "/api/v1/plugins").await;
    let requests_before = provider.requests().len();
    let path = format!(
        "/api/v1/recordings/{}/artist-associations/{}/edition-track-list",
        imported.recording,
        association["id"].as_str().unwrap(),
    );
    let body = json!({"release_group_id": catalog::GROUP, "edition_id": catalog::EDITION});
    let (status, headers, _) =
        review::exchange(app, "POST", &path, body.clone(), peer.headers()).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    private(&headers);
    assert_eq!(provider.requests().len(), requests_before);
    let (status, headers, response) =
        review::exchange(app, "POST", &path, body.clone(), account.headers()).await;
    assert_eq!(status, StatusCode::OK);
    private(&headers);
    assert_eq!(response["status"], "available");
    assert_eq!(response["association_id"], association["id"]);
    assert_eq!(response["edition"], json!(expected_edition));
    assert_eq!(response["linked"], json!([]));
    assert_eq!(imported.snapshot(app, account).await, before);
    assert_eq!(review::origin_counts(origin).await, origin_before);
    assert_eq!(peer.get(app, "/api/v1/recordings").await, peer_before);
    assert_eq!(peer.get(app, "/api/v1/plugins").await, peer_plugins);
    assert_eq!(provider.unexpected(), 0);
    let requests = provider.requests();
    let dispatched = &requests[requests_before..];
    assert_eq!(dispatched.len(), 2);
    assert_eq!(dispatched[0].operation, "v1/catalog/editions/capabilities");
    assert_eq!(dispatched[1].operation, catalog::TRACK_LIST);
    assert_eq!(dispatched[1].json(), body);
    assert!(
        dispatched
            .iter()
            .all(|request| request.protocol().as_deref() == Some("1"))
    );
    response
}
