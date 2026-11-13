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
use hito_testkit::fake_plugin::{FakePlugin, Reply};
use serde_json::{Value, json};
use std::{
    panic::{AssertUnwindSafe, resume_unwind},
    sync::Arc,
    time::Duration,
};

const TOKEN: &str = "owned-group-credential-token";

#[tokio::test]
async fn protected_group_failures_keep_their_current_stamp_until_a_valid_detail_recovers() {
    let directory = Arc::new(tempfile::tempdir().unwrap());
    let music = directory.path().join("music");
    std::fs::create_dir(&music).unwrap();
    let audio = music.join("owned-group-credential-original.wav");
    support::write_wav(&audio);
    let original = std::fs::read(&audio).unwrap();
    let owner = DatabaseOwner::retain(directory.clone());
    let mut config = CoreConfig::new(
        directory.path().join("hito.db"),
        music,
        HostDeviceId::new("group-credential-http").unwrap(),
    );
    owner.attach(&mut config);
    let services = hang_guard(
        "group credential owned services",
        Box::pin(hito_core::open(config)),
    )
    .await
    .unwrap();
    let app = hito_server::router(services.clone());
    let origin = hang_guard(
        "group credential owned origin",
        Box::pin(ProviderServer::start()),
    )
    .await;
    let mut provider =
        hang_guard("group credential owned catalog", Box::pin(catalog::start())).await;
    let outcome = AssertUnwindSafe(Box::pin(async {
        let reviewed = hang_guard(
            "group credential reviewed association",
            Box::pin(prepare(&app, &origin, &provider)),
        )
        .await;
        tokio::time::timeout(
            Duration::from_secs(30),
            credential_transitions(&app, &reviewed, &origin, &provider),
        )
        .await
        .expect("The group credential workflow must finish within its deadline");
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
    if let Err(panic) = outcome {
        resume_unwind(panic);
    }
}

async fn credential_transitions(
    app: &Router,
    reviewed: &Reviewed,
    origin: &ProviderServer,
    provider: &FakePlugin,
) {
    let Reviewed {
        account,
        peer,
        imported,
        association,
    } = reviewed;
    let id = association["provider"]["installation_id"].as_str().unwrap();
    let settings = account.get(app, "/api/v1/plugins").await;
    let previous = installation(&settings, id);
    let service = settings["services"]
        .as_array()
        .unwrap()
        .iter()
        .find(|service| service["id"] == previous["service_id"])
        .unwrap();
    let (status, peer_installation) = peer.request(app, "POST", "/api/v1/plugins", json!({
        "service_id": service["id"], "service_revision": service["revision"], "bearer_token": null,
    })).await;
    assert_eq!(status, StatusCode::CREATED);
    assert_ne!(peer_installation["id"], previous["id"]);
    let (status, refreshed) = account.request(app, "POST", &format!("/api/v1/plugins/{id}/refresh"), json!({
        "revision": previous["revision"], "credential": {"action": "replace", "bearer_token": TOKEN},
    })).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(refreshed["kind"], "updated");
    let current = &refreshed["installation"];
    assert_eq!(current["id"], previous["id"]);
    assert_eq!(
        current["revision"].as_i64().unwrap(),
        previous["revision"].as_i64().unwrap() + 1
    );
    assert_eq!(current["credential_set"], true);
    provider.require_bearer(TOKEN);
    let before = imported.snapshot(app, account).await;
    let origin_before = review::origin_counts(origin).await;
    let peer_before = peer.get(app, "/api/v1/plugins").await;
    let peer_recordings = peer.get(app, "/api/v1/recordings").await;
    let requests_before = provider.requests().len();
    let path = format!(
        "/api/v1/recordings/{}/artist-associations/{}/release-group",
        imported.recording,
        association["id"].as_str().unwrap()
    );
    let body = json!({"release_group_id": catalog::GROUP});
    provider.reply(
        catalog::GROUP_CAPABILITIES,
        Reply::Status(StatusCode::UNAUTHORIZED),
    );
    let descriptor_failure =
        review::exchange(app, "POST", &path, body.clone(), account.headers()).await;
    rejected(&descriptor_failure, current);
    let descriptor_settings = account.get(app, "/api/v1/plugins").await;
    assert_eq!(
        installation(&descriptor_settings, id)["health"]["state"],
        "credentials_required"
    );
    assert!(provider.requests_for(catalog::GROUP_DETAIL).is_empty());
    assert_eq!(peer.get(app, "/api/v1/plugins").await, peer_before);
    let (status, headers, denied) =
        review::exchange(app, "POST", &path, body.clone(), peer.headers()).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    private(&headers);
    assert!(denied.get("plugin_connection").is_none());
    provider.reply(
        catalog::GROUP_CAPABILITIES,
        Reply::Json(json!({"release_group_version": 1, "operations": ["release_group"]})),
    );
    provider.reply(
        catalog::GROUP_DETAIL,
        Reply::Status(StatusCode::UNAUTHORIZED),
    );
    let detail_failure =
        review::exchange(app, "POST", &path, body.clone(), account.headers()).await;
    rejected(&detail_failure, current);
    let rejected_settings = account.get(app, "/api/v1/plugins").await;
    assert_eq!(rejected_settings, descriptor_settings);
    provider.reply(
        catalog::GROUP_CAPABILITIES,
        Reply::Status(StatusCode::NOT_FOUND),
    );
    let (status, headers, unsupported) =
        review::exchange(app, "POST", &path, body.clone(), account.headers()).await;
    assert_eq!(status, StatusCode::OK);
    private(&headers);
    assert_eq!(unsupported, json!({"status": "unsupported"}));
    assert_eq!(account.get(app, "/api/v1/plugins").await, rejected_settings);
    provider.reply(
        catalog::GROUP_CAPABILITIES,
        Reply::Json(json!({"release_group_version": 1, "operations": ["release_group"]})),
    );
    provider.reply(catalog::GROUP_DETAIL, Reply::Json(json!(catalog::group())));
    let (status, headers, recovered) =
        review::exchange(app, "POST", &path, body, account.headers()).await;
    assert_eq!(status, StatusCode::OK);
    private(&headers);
    assert_eq!(
        recovered,
        json!({"status": "available", "association_id": association["id"], "group": catalog::group()})
    );
    let recovered_settings = account.get(app, "/api/v1/plugins").await;
    assert!(
        installation(&recovered_settings, id)
            .get("health")
            .is_none()
    );
    assert_eq!(installation(&recovered_settings, id), current);
    assert_eq!(imported.snapshot(app, account).await, before);
    assert_eq!(review::origin_counts(origin).await, origin_before);
    assert_eq!(peer.get(app, "/api/v1/plugins").await, peer_before);
    assert_eq!(peer.get(app, "/api/v1/recordings").await, peer_recordings);
    assert_eq!(provider.unexpected(), 0);
    let requests = provider.requests();
    let dispatched = &requests[requests_before..];
    assert_eq!(
        dispatched
            .iter()
            .map(|request| request.operation.as_str())
            .collect::<Vec<_>>(),
        [
            catalog::GROUP_CAPABILITIES,
            catalog::GROUP_CAPABILITIES,
            catalog::GROUP_DETAIL,
            catalog::GROUP_CAPABILITIES,
            catalog::GROUP_CAPABILITIES,
            catalog::GROUP_DETAIL,
        ]
    );
    for request in dispatched {
        assert!(
            request
                .headers
                .get("authorization")
                .is_some_and(|value| value == format!("Bearer {TOKEN}").as_str())
        );
        assert_eq!(request.protocol().as_deref(), Some("1"));
        if request.operation == catalog::GROUP_DETAIL {
            assert_eq!(request.json(), json!({"release_group_id": catalog::GROUP}));
        }
    }
}

fn installation<'a>(settings: &'a Value, id: &str) -> &'a Value {
    settings["installations"]
        .as_array()
        .unwrap()
        .iter()
        .find(|installation| installation["id"] == id)
        .unwrap()
}

fn rejected(response: &(StatusCode, HeaderMap, Value), current: &Value) {
    let (status, headers, body) = response;
    assert_eq!(*status, StatusCode::CONFLICT);
    private(headers);
    assert_eq!(body["code"], "plugin_credentials_required");
    assert_eq!(
        body["plugin_connection"],
        json!({
            "installation_id": current["id"], "installation_revision": current["revision"],
        })
    );
    assert_eq!(body.as_object().unwrap().len(), 3);
    assert!(!body.to_string().contains(TOKEN));
}
