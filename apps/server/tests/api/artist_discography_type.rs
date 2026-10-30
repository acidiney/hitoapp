use hito_core::test_support::artist_catalog_discography_service as catalog;

use crate::{
    artist_review_fixture::{self as fixture, Account, Imported},
    provider_fixture::ProviderServer,
    support,
};
use axum::http::StatusCode;
use serde_json::{Value, json};

fn body(release_type: Value, continuation: Value) -> Value {
    json!({"limit": 1, "continuation": continuation, "release_type": release_type})
}

#[tokio::test]
async fn an_http_account_pages_one_release_type_and_learns_when_the_provider_cannot() {
    let (_directory, app) = support::app().await;
    let account = Account::setup(&app).await;
    let origin = ProviderServer::start().await;
    let mut provider = catalog::CatalogServer::start().await;
    let imported = Imported::create(&app, &account, &origin).await;
    let installation = account
        .connect(&app, &provider.url, "Owned typed discography", None)
        .await;
    let selected = json!({"kind": "http_plugin", "installation_id": installation["id"]});
    let settings = account.get(&app, "/api/v1/metadata/settings").await;
    let (status, _) = account
        .request(
            &app,
            "PUT",
            "/api/v1/metadata/settings",
            json!({"revision": settings["revision"], "enabled": true, "providers": [selected]}),
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    let review = format!("/api/v1/recordings/{}/artist-review", imported.recording);
    let (status, context) = account
        .request(
            &app,
            "POST",
            &format!("{review}/context"),
            json!({"provider": selected}),
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    let selection = json!({"context": context, "provider_artist_id": catalog::ARTIST_ID});
    let (status, _) = account
        .request(
            &app,
            "POST",
            &format!("{review}/preview"),
            selection.clone(),
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    let (status, confirmed) = account
        .request(&app, "POST", &format!("{review}/confirm"), selection)
        .await;
    assert_eq!(status, StatusCode::OK);
    let association = confirmed["id"].as_str().unwrap();
    let path = format!(
        "/api/v1/recordings/{}/artist-associations/{association}/discography",
        imported.recording
    );
    provider.enable_discography();

    let (status, unsupported) = account
        .request(&app, "POST", &path, body(json!("album"), Value::Null))
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(unsupported, json!({"status": "type_filter_unsupported"}));

    provider.enable_type_filter();
    let (status, headers, first) = fixture::exchange(
        &app,
        "POST",
        &path,
        body(json!("album"), Value::Null),
        account.headers(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(headers.get("cache-control").unwrap(), "private, no-store");
    assert_eq!(first["status"], "available");
    assert_eq!(first["page"]["items"][0]["kind"], "album");
    let continuation = first["continuation"].clone();
    assert!(continuation.is_string());

    for mismatched in [
        body(json!("ep"), continuation.clone()),
        body(Value::Null, continuation.clone()),
    ] {
        let (status, _) = account.request(&app, "POST", &path, mismatched).await;
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    }
    for malformed in [
        body(json!("broadcast"), Value::Null),
        body(json!(["album"]), Value::Null),
    ] {
        let (status, _) = account.request(&app, "POST", &path, malformed).await;
        assert!(
            matches!(
                status,
                StatusCode::BAD_REQUEST | StatusCode::UNPROCESSABLE_ENTITY
            ),
            "an unknown or malformed release type must be rejected before provider work"
        );
    }
    let (status, last) = account
        .request(&app, "POST", &path, body(json!("album"), continuation))
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        last["page"]["items"][0]["id"],
        "opaque:release-group/album-2"
    );
    assert!(last["continuation"].is_null());

    let (status, whole) = account
        .request(
            &app,
            "POST",
            &path,
            json!({"limit": 1, "continuation": null}),
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(whole["status"], "available");

    provider.fail_typed_reads(true);
    let (status, outage) = account
        .request(&app, "POST", &path, body(json!("album"), Value::Null))
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(outage, json!({"status": "unavailable"}));
    assert_eq!(provider.unexpected(), 0);
    origin.shutdown().await;
    assert!(provider.finish().await);
}
