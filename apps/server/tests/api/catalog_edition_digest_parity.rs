use crate::{
    account::exchange,
    catalog_edition_digest_parity_fixture::{self as fixture, Context, Library, accepted},
};
use axum::http::{Method, StatusCode};
use futures_util::FutureExt;
use hito_core::test_support::{
    album_edition_service::{self as catalog, AlbumProvider},
    hang_guard::hang_guard,
};
use hito_plugin_sdk::{
    AUDIO_PATH, CatalogEditionTrackList, EditionTrackListRequest, PREPARATION_STATUS_PATH,
    PREPARE_SOURCE_PATH, SOURCES_PATH,
};
use serde_json::{Value, json};
use std::{
    panic::{AssertUnwindSafe, resume_unwind},
    time::Duration,
};
use uuid::Uuid;

const EDITION_DIGEST: &str = "ce9caa59b2ef317a582ae0d58b8f251e48e5d6ceaa8eee5b1d38d48ea29a0093";

#[tokio::test]
async fn the_same_selected_edition_has_one_digest_across_catalog_explicit_and_saved_reads() {
    let mut library = Library::new();
    let mut context = None;
    let outcome = AssertUnwindSafe(Box::pin(async {
        context = Some(
            hang_guard(
                "edition digest parity owned setup",
                Box::pin(library.prepare()),
            )
            .await,
        );
        tokio::time::timeout(
            Duration::from_secs(30),
            exercise(
                context.as_ref().unwrap(),
                library.provider.as_ref().unwrap(),
            ),
        )
        .await
        .expect("The public digest parity workflow must finish within its deadline")
    }))
    .catch_unwind()
    .await;
    drop(context.take());
    library.retire().await;
    let (responses, needs_choice) = match outcome {
        Ok(responses) => responses,
        Err(panic) => resume_unwind(panic),
    };
    assert_eq!(needs_choice, json!({"status": "needs_choice"}));
    assert!(needs_choice.get("edition_digest").is_none());
    for (route, response) in ["generic", "explicit", "saved"].into_iter().zip(responses) {
        assert_eq!(
            response.get("edition_digest"),
            Some(&json!(EDITION_DIGEST)),
            "The {route} read must carry the same versioned digest of the exact SDK edition"
        );
    }
}

async fn exercise(owned: &Context, provider: &AlbumProvider) -> ([Value; 3], Value) {
    let Context {
        app,
        account,
        peer,
        prepared,
        ..
    } = owned;
    let edition = catalog::track_list(catalog::EDITION_ID).unwrap();
    serde_json::from_value::<CatalogEditionTrackList>(edition.clone())
        .unwrap()
        .validate_for(&EditionTrackListRequest {
            release_group_id: catalog::GROUP_ID.into(),
            edition_id: catalog::EDITION_ID.into(),
        })
        .unwrap();
    let generic = format!(
        "/api/v1/recordings/{}/artist-associations/{}/edition-track-list",
        prepared.first, prepared.association
    );
    let explicit = format!("/api/v1/albums/{}/edition-track-list", prepared.release);
    let saved = format!("/api/v1/albums/{}/edition/track-list", prepared.release);
    let choose = format!("/api/v1/albums/{}/edition", prepared.release);
    let target = json!({"recording_id": prepared.first, "association_id": prepared.association,
        "release_group_id": catalog::GROUP_ID, "edition_id": catalog::EDITION_ID});
    let choice_id = Uuid::new_v4();
    let mut choice = target.clone();
    choice["request_id"] = json!(choice_id);
    choice["expected_revision"] = json!(0);
    let generic_body =
        json!({"release_group_id": catalog::GROUP_ID, "edition_id": catalog::EDITION_ID});
    let before = owned.snapshot().await;
    let peer_before = owned.peer_snapshot().await;
    assert_eq!(peer_before["recordings"]["total"], 0);
    assert_eq!(peer_before["plugins"]["installations"], json!([]));
    assert_eq!(
        before["album"]["saved_edition"],
        json!({"revision": 0, "state": {"status": "unset"}})
    );
    let requests_before = provider.requests().len();
    for (method, path, body) in [
        ("POST", &generic, generic_body.clone()),
        ("POST", &explicit, target.clone()),
        ("POST", &saved, Value::Null),
        ("PUT", &choose, choice.clone()),
    ] {
        let (status, headers, denied) = exchange(app, method, path, body, peer.headers()).await;
        assert_eq!(status, StatusCode::NOT_FOUND);
        fixture::private(&headers);
        assert_eq!(denied, json!({"error": "Resource not found"}));
    }
    let needs_choice = accepted(app, account, "POST", &saved, Value::Null).await;
    assert_eq!(needs_choice["status"], "needs_choice");
    assert_eq!(provider.requests().len(), requests_before);
    let receipt = accepted(app, account, "PUT", &choose, choice).await;
    assert_eq!(
        receipt,
        json!({"request_id": choice_id, "release_id": prepared.release,
        "revision": 1, "edition": target})
    );
    assert_eq!(
        provider.requests().len(),
        requests_before,
        "Saving a choice must not contact the provider"
    );
    let mut chosen = before;
    chosen["album"]["saved_edition"] = json!({"revision": 1,
        "state": {"status": "ready", "edition": target}});
    assert_eq!(owned.snapshot().await, chosen);
    let generic_response = accepted(app, account, "POST", &generic, generic_body.clone()).await;
    let explicit_response = accepted(app, account, "POST", &explicit, target).await;
    let saved_response = accepted(app, account, "POST", &saved, Value::Null).await;
    for response in [&generic_response, &explicit_response, &saved_response] {
        assert_eq!(response["status"], "available");
        assert_eq!(response["association_id"], prepared.association);
        assert_eq!(response["edition"], edition);
        assert_eq!(
            response["stats"],
            json!({"audio_tracks": 6, "total_duration_ms": 6000})
        );
    }
    assert_eq!(generic_response["linked"], json!([]));
    for response in [&explicit_response, &saved_response] {
        assert_eq!(
            response["matches"]
                .as_array()
                .unwrap()
                .iter()
                .map(|matched| (
                    matched["occurrence_id"].clone(),
                    matched["recording"]["id"].clone()
                ))
                .collect::<Vec<_>>(),
            vec![
                (json!("occurrence:album/1/1"), json!(prepared.first)),
                (
                    json!("occurrence:album/1/2"),
                    prepared.saved[1]["id"].clone()
                ),
                (json!("occurrence:album/2/1"), json!(prepared.first)),
            ]
        );
        assert_eq!(
            response["matches"][1]["recording"]["title"],
            fixture::PRIVATE_TITLE
        );
        assert_eq!(
            response["edition"]["media"][0]["tracks"][2]["title"],
            "Second catalog title"
        );
        assert_eq!(response["library_only"].as_array().unwrap().len(), 1);
        assert_eq!(response["library_only"][0]["id"], prepared.saved[2]["id"]);
        assert_eq!(response["attached_occurrences"], json!([]));
        assert_eq!(response["is_album_release"], true);
        assert_eq!(
            response["adoption"],
            json!({"installation_id": prepared.installation["id"],
            "installation_revision": prepared.installation["revision"]})
        );
    }
    let mut explicit_projection = explicit_response.clone();
    let mut saved_projection = saved_response.clone();
    explicit_projection
        .as_object_mut()
        .unwrap()
        .remove("edition_digest");
    saved_projection
        .as_object_mut()
        .unwrap()
        .remove("edition_digest");
    assert_eq!(explicit_projection, saved_projection);
    assert_eq!(owned.snapshot().await, chosen);
    assert_eq!(owned.peer_snapshot().await, peer_before);
    let requests = provider.requests();
    let dispatched = &requests[requests_before..];
    assert_eq!(dispatched.len(), 6);
    for pair in dispatched.as_chunks::<2>().0 {
        assert_eq!(pair[0].method, Method::GET);
        assert_eq!(pair[0].path, "/prefix/v1/catalog/editions/capabilities");
        assert_eq!(pair[1].method, Method::POST);
        assert_eq!(pair[1].path, "/prefix/v1/catalog/editions/track-list");
        assert_eq!(pair[1].body, generic_body);
    }
    assert!(
        dispatched
            .iter()
            .all(|request| request.headers["x-hito-plugin-protocol"] == "1")
    );
    for path in [
        SOURCES_PATH,
        AUDIO_PATH,
        PREPARE_SOURCE_PATH,
        PREPARATION_STATUS_PATH,
    ] {
        assert!(provider.requests_to(path).is_empty());
    }
    assert_eq!(provider.unexpected(), 0);
    (
        [generic_response, explicit_response, saved_response],
        needs_choice,
    )
}
