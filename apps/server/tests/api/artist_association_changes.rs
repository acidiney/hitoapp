use crate::{
    artist_review_fixture::{self as fixture, Account, Imported, catalog},
    provider_fixture::ProviderServer,
    support,
};
use axum::{Router, http::StatusCode};
use futures_util::FutureExt;
use hito_core::MetadataProvider;
use serde_json::{Value, json};
use std::{
    panic::{AssertUnwindSafe, resume_unwind},
    time::Duration,
};
use uuid::Uuid;

#[tokio::test]
async fn http_unlink_and_undo_change_only_the_callers_saved_artist_review() {
    let (_directory, app) = support::app().await;
    let account = Account::setup(&app).await;
    let origin = ProviderServer::start().await;
    let mut provider = catalog::DiscoveryServer::start().await;
    let result = tokio::time::timeout(
        Duration::from_secs(30),
        AssertUnwindSafe(exercise(&app, &account, &origin, &provider)).catch_unwind(),
    )
    .await;
    origin.shutdown().await;
    let joined = provider.finish().await;
    match result {
        Ok(Ok(())) => {}
        Ok(Err(panic)) => resume_unwind(panic),
        Err(_) => panic!("The owned HTTP association change workflow must finish in time"),
    }
    assert!(joined, "The owned catalog provider must join shutdown");
}

async fn exercise(
    app: &Router,
    account: &Account,
    origin: &ProviderServer,
    provider: &catalog::DiscoveryServer,
) {
    let imported = Imported::create(app, account, origin).await;
    let installation = account
        .connect(
            app,
            &provider.url,
            "Owned change catalog",
            Some(catalog::TOKEN),
        )
        .await;
    let selected = MetadataProvider::HttpPlugin {
        installation_id: installation["id"].as_str().unwrap().into(),
    };
    let settings = account.get(app, "/api/v1/metadata/settings").await;
    let (status, _) = account
        .request(
            app,
            "PUT",
            "/api/v1/metadata/settings",
            json!({"revision": settings["revision"], "enabled": true, "providers": [selected]}),
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    let (status, _) = account
        .request(app, "POST", "/api/v1/users", json!({
            "username": "other-change-reviewer", "password": "owned other change reviewer password",
        }))
        .await;
    assert_eq!(status, StatusCode::CREATED);
    let other = Account::login(
        app,
        "other-change-reviewer",
        "owned other change reviewer password",
    )
    .await;
    let base = format!("/api/v1/recordings/{}", imported.recording);
    let context = accepted(
        app,
        account,
        "POST",
        &format!("{base}/artist-review/context"),
        json!({"provider": selected}),
    )
    .await;
    let confirmed = accepted(
        app,
        account,
        "POST",
        &format!("{base}/artist-review/confirm"),
        json!({"context": context, "provider_artist_id": catalog::SECOND_ARTIST}),
    )
    .await;
    let association = confirmed["id"].as_str().unwrap().to_owned();
    let changes = format!("{base}/artist-association-changes");
    let unlink = format!("{base}/artist-associations/unlink");
    let undo = format!("{base}/artist-associations/undo");
    let profile = format!("{base}/artist-associations/{association}/profile");

    let state = accepted(app, account, "GET", &changes, Value::Null).await;
    assert_eq!(state["revision"], 1);
    assert_eq!(state["undo"]["association_id"], association);
    assert_eq!(state["undo"]["linked"], false);

    let request = Uuid::new_v4();
    let body =
        json!({"request_id": request, "expected_revision": 1, "association_id": association});
    let mut anonymous = account.headers();
    anonymous.remove("cookie");
    let mut foreign = account.headers();
    foreign.insert("x-hito-account", other.id.parse().unwrap());
    for (headers, expected) in [
        (anonymous, StatusCode::UNAUTHORIZED),
        (foreign, StatusCode::FORBIDDEN),
    ] {
        for (method, path) in [("GET", &changes), ("POST", &unlink), ("POST", &undo)] {
            let (status, _, _) =
                fixture::exchange(app, method, path, body.clone(), headers.clone()).await;
            assert_eq!(status, expected);
        }
    }
    let (status, _) = other.request(app, "POST", &unlink, body.clone()).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let mut unknown = body.clone();
    unknown["unexpected_private_field"] = json!(true);
    let (status, _) = account.request(app, "POST", &unlink, unknown).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);

    let receipt = accepted(app, account, "POST", &unlink, body.clone()).await;
    assert_eq!(receipt["revision"], 2);
    assert_eq!(receipt["linked"], false);
    assert_eq!(receipt["association_id"], association);
    assert_eq!(
        accepted(app, account, "POST", &unlink, body.clone()).await,
        receipt
    );
    let associations = format!("{base}/artist-associations");
    assert_eq!(
        accepted(app, account, "GET", &associations, Value::Null).await,
        json!([])
    );
    let requests = provider.requests().len();
    let (status, _) = account.request(app, "GET", &profile, Value::Null).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(provider.requests().len(), requests);
    let (status, _) = account
        .request(
            app,
            "POST",
            &unlink,
            json!({"request_id": Uuid::new_v4(), "expected_revision": 1, "association_id": association}),
        )
        .await;
    assert_eq!(status, StatusCode::CONFLICT);

    let undo_body = json!({"request_id": Uuid::new_v4(), "expected_revision": 2});
    let restored = accepted(app, account, "POST", &undo, undo_body.clone()).await;
    assert_eq!(restored["revision"], 3);
    assert_eq!(restored["linked"], true);
    assert_eq!(
        accepted(app, account, "POST", &undo, undo_body).await,
        restored
    );
    assert_eq!(
        accepted(app, account, "GET", &associations, Value::Null).await,
        json!([confirmed])
    );
    let available = accepted(app, account, "GET", &profile, Value::Null).await;
    assert_eq!(available["status"], "available");
    assert_eq!(provider.unexpected(), 0);
}

async fn accepted(app: &Router, account: &Account, method: &str, path: &str, body: Value) -> Value {
    let (status, _, value) = fixture::exchange(app, method, path, body, account.headers()).await;
    assert_eq!(status, StatusCode::OK);
    value
}
