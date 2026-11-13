use super::{Reviewed, prepare, private};
use crate::{
    artist_review_fixture as review, catalog_album_header_fixture as catalog,
    provider_fixture::ProviderServer, support,
};
use axum::{
    Router,
    http::{HeaderMap, StatusCode},
};
use futures_util::FutureExt;
use hito_core::{
    CoreConfig, HostDeviceId,
    test_support::{database_owner::DatabaseOwner, hang_guard::hang_guard},
};
use hito_testkit::fake_plugin::FakePlugin;
use serde_json::{Value, json};
use std::{
    panic::{AssertUnwindSafe, resume_unwind},
    sync::Arc,
    time::Duration,
};

#[tokio::test]
async fn a_held_optional_group_does_not_block_the_same_accounts_edition_tracks() {
    let directory = Arc::new(tempfile::tempdir().unwrap());
    let music = directory.path().join("music");
    std::fs::create_dir(&music).unwrap();
    let audio = music.join("owned-parallel-header-original.wav");
    support::write_wav(&audio);
    let original = std::fs::read(&audio).unwrap();
    let owner = DatabaseOwner::retain(directory.clone());
    let mut config = CoreConfig::new(
        directory.path().join("hito.db"),
        music,
        HostDeviceId::new("parallel-header-http").unwrap(),
    );
    owner.attach(&mut config);
    let services = hang_guard(
        "parallel header owned services",
        Box::pin(hito_core::open(config)),
    )
    .await
    .unwrap();
    let app = hito_server::router(services.clone());
    let origin = hang_guard(
        "parallel header owned origin",
        Box::pin(ProviderServer::start()),
    )
    .await;
    let mut provider =
        hang_guard("parallel header owned catalog", Box::pin(catalog::start())).await;
    let outcome = AssertUnwindSafe(Box::pin(async {
        let reviewed = hang_guard(
            "parallel header reviewed association",
            Box::pin(prepare(&app, &origin, &provider)),
        )
        .await;
        tokio::time::timeout(
            Duration::from_secs(30),
            parallel_reads(&app, &reviewed, &origin, &provider),
        )
        .await
        .expect("The parallel catalog workflow must finish within its deadline")
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
    let (status, headers, tracks, dispatched) = match outcome {
        Ok(response) => response,
        Err(panic) => resume_unwind(panic),
    };
    assert_eq!(
        status,
        StatusCode::OK,
        "Optional group metadata must not occupy the same account's edition-track-list admission"
    );
    private(&headers);
    assert_eq!(tracks["status"], "available");
    assert_eq!(tracks["edition"], json!(catalog::edition()));
    assert_eq!(tracks["linked"], json!([]));
    assert_eq!(
        dispatched,
        [
            catalog::GROUP_CAPABILITIES,
            catalog::GROUP_DETAIL,
            "v1/catalog/editions/capabilities",
            catalog::TRACK_LIST,
        ]
    );
}

async fn parallel_reads(
    app: &Router,
    reviewed: &Reviewed,
    origin: &ProviderServer,
    provider: &FakePlugin,
) -> (StatusCode, HeaderMap, Value, Vec<String>) {
    let Reviewed {
        account,
        peer,
        imported,
        association,
    } = reviewed;
    let before = imported.snapshot(app, account).await;
    let origin_before = review::origin_counts(origin).await;
    let peer_before = peer.get(app, "/api/v1/recordings").await;
    let requests_before = provider.requests().len();
    let path = format!(
        "/api/v1/recordings/{}/artist-associations/{}",
        imported.recording,
        association["id"].as_str().unwrap()
    );
    let gate = provider.hold_next(catalog::GROUP_DETAIL);
    let group = {
        let app = app.clone();
        let path = format!("{path}/release-group");
        let headers = account.headers();
        tokio::spawn(async move {
            review::exchange(
                &app,
                "POST",
                &path,
                json!({"release_group_id": catalog::GROUP}),
                headers,
            )
            .await
        })
    };
    let mut entered = false;
    let concurrent = AssertUnwindSafe(Box::pin(async {
        gate.entered().await;
        entered = true;
        assert!(!group.is_finished());
        let tracks = review::exchange(
            app,
            "POST",
            &format!("{path}/edition-track-list"),
            json!({"release_group_id": catalog::GROUP, "edition_id": catalog::EDITION}),
            account.headers(),
        )
        .await;
        assert!(
            !group.is_finished(),
            "The edition request must complete while the group is still held"
        );
        tracks
    }))
    .catch_unwind()
    .await;
    gate.open();
    let group_result = group.await;
    let gate_retired = if entered {
        gate.retired_within(Duration::from_secs(5)).await
    } else {
        true
    };
    let (status, headers, tracks) = match concurrent {
        Ok(response) => response,
        Err(panic) => resume_unwind(panic),
    };
    let (group_status, group_headers, group_body) = group_result.unwrap();
    assert!(gate_retired);
    assert_eq!(group_status, StatusCode::OK);
    private(&group_headers);
    assert_eq!(
        group_body,
        json!({"status": "available", "association_id": association["id"], "group": catalog::group()})
    );
    assert_eq!(imported.snapshot(app, account).await, before);
    assert_eq!(review::origin_counts(origin).await, origin_before);
    assert_eq!(peer.get(app, "/api/v1/recordings").await, peer_before);
    assert_eq!(provider.unexpected(), 0);
    let dispatched = provider.requests()[requests_before..]
        .iter()
        .map(|request| request.operation.clone())
        .collect();
    (status, headers, tracks, dispatched)
}
