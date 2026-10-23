use crate::{
    artist_review_fixture::{self as fixture, Account, Imported, catalog},
    provider_fixture::ProviderServer,
    support,
};
use axum::{Router, http::StatusCode};
use futures_util::FutureExt;
use hito_core::{Action, ArtistReviewContext, MetadataProvider, Operation};
use hito_plugin_sdk::ArtistSearchRequest;
use serde_json::{Value, json};
use std::{
    panic::{AssertUnwindSafe, resume_unwind},
    time::Duration,
};
use uuid::Uuid;

#[tokio::test]
async fn http_artist_review_rejects_foreign_or_stale_context_without_private_changes() {
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
        Err(_) => {
            panic!("The owned HTTP artist authority workflow must finish within its deadline")
        }
    }
    assert!(
        joined,
        "The owned catalog provider must close and join shutdown"
    );
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
            "Owned authority catalog",
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
            json!({
                "revision": settings["revision"], "enabled": true, "providers": [selected],
            }),
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    let (status, user) = account.request(app, "POST", "/api/v1/users", json!({
        "username": "other-artist-reviewer", "password": "owned other artist reviewer password",
    })).await;
    assert_eq!(status, StatusCode::CREATED);
    let other = Account::login(
        app,
        "other-artist-reviewer",
        "owned other artist reviewer password",
    )
    .await;
    assert!(json!(other.id) == user["id"] && other.id != account.id);
    let review = format!("/api/v1/recordings/{}/artist-review", imported.recording);
    let other_review = format!(
        "/api/v1/recordings/{}/artist-review",
        imported.other_recording
    );
    let associations = format!(
        "/api/v1/recordings/{}/artist-associations",
        imported.recording
    );
    let other_associations = format!(
        "/api/v1/recordings/{}/artist-associations",
        imported.other_recording
    );
    let context = accepted(
        app,
        account,
        "POST",
        &format!("{review}/context"),
        json!({"provider": selected}),
    )
    .await;
    let captured: ArtistReviewContext = serde_json::from_value(context.clone()).unwrap();
    assert!(captured.account_id == account.id && captured.artist_id == imported.artist);
    assert!(captured.observation_id == imported.observation && captured.source_id.is_none());
    let search = json!({"context": context, "search": ArtistSearchRequest {
        query: fixture::CREDIT.into(), cursor: None, snapshot: None, limit: 2,
    }});
    let selection = json!({"context": context, "provider_artist_id": catalog::SECOND_ARTIST});
    let before = imported.snapshot(app, account).await;
    let origin_before = fixture::origin_counts(origin).await;
    assert_eq!(provider.requests().len(), 1);

    let mut anonymous = account.headers();
    anonymous.remove("cookie");
    let mut foreign_header = account.headers();
    foreign_header.insert("x-hito-account", other.id.parse().unwrap());
    for (method, path, body) in [
        (
            "POST",
            format!("{review}/context"),
            json!({"provider": selected}),
        ),
        ("POST", format!("{review}/search"), search.clone()),
        ("POST", format!("{review}/preview"), selection.clone()),
        ("POST", format!("{review}/confirm"), selection.clone()),
        ("GET", associations.clone(), Value::Null),
        (
            "GET",
            format!("{associations}/{}/profile", Uuid::new_v4()),
            Value::Null,
        ),
    ] {
        for (headers, expected) in [
            (anonymous.clone(), StatusCode::UNAUTHORIZED),
            (foreign_header.clone(), StatusCode::FORBIDDEN),
        ] {
            let (status, _, _) = fixture::exchange(app, method, &path, body.clone(), headers).await;
            assert_eq!(status, expected);
        }
    }
    let mut foreign_origin = account.headers();
    foreign_origin.insert("origin", "https://untrusted.example".parse().unwrap());
    let mut missing_guard = account.headers();
    missing_guard.remove("x-hito-request");
    for headers in [foreign_origin, missing_guard] {
        let (status, _, _) = fixture::exchange(
            app,
            "POST",
            &format!("{review}/context"),
            json!({"provider": selected}),
            headers,
        )
        .await;
        assert_eq!(status, StatusCode::FORBIDDEN);
    }
    for (suffix, body) in [
        ("search", search.clone()),
        ("preview", selection.clone()),
        ("confirm", selection.clone()),
    ] {
        let (status, _) = account
            .request(app, "POST", &format!("{other_review}/{suffix}"), body)
            .await;
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    }
    let mut unknown_context = selection.clone();
    unknown_context["context"]["unexpected_private_field"] = json!(true);
    let mut unknown_search = search;
    unknown_search["search"]["unexpected_private_field"] = json!(true);
    for (suffix, body) in [("preview", unknown_context), ("search", unknown_search)] {
        let (status, _) = account
            .request(app, "POST", &format!("{review}/{suffix}"), body)
            .await;
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    }
    let mut forged = selection.clone();
    forged["context"]["account_id"] = json!(other.id);
    let (status, _) = account
        .request(app, "POST", &format!("{review}/confirm"), forged)
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let (status, _) = other
        .request(app, "POST", &format!("{review}/confirm"), selection.clone())
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(
        provider.requests().len(),
        1,
        "Rejected requests must not contact the catalog provider"
    );
    assert!(imported.snapshot(app, account).await == before);
    assert!(accepted(app, account, "GET", &associations, Value::Null).await == json!([]));
    assert!(accepted(app, account, "GET", &other_associations, Value::Null).await == json!([]));
    assert_eq!(fixture::origin_counts(origin).await, origin_before);

    let preview = accepted(
        app,
        account,
        "POST",
        &format!("{review}/preview"),
        selection.clone(),
    )
    .await;
    assert!(preview["status"] == "available" && preview["context"] == context);
    assert!(preview["profile"] == json!(catalog::expected_profile()));
    assert_eq!(provider.requests().len(), 3);
    let recording = Uuid::parse_str(&imported.recording).unwrap();
    let operations = [
        Action::RecordingArtistSet {
            recording_id: recording,
            artist: "Private intervening HTTP artist".into(),
        },
        Action::RecordingArtistSet {
            recording_id: recording,
            artist: fixture::CREDIT.into(),
        },
    ]
    .into_iter()
    .map(|action| Operation {
        id: Uuid::new_v4(),
        action,
    })
    .collect::<Vec<_>>();
    let (status, acknowledged) = account
        .request(
            app,
            "POST",
            "/api/v1/sync",
            json!({
                "device_id": "owned-http-artist-review-aba", "operations": operations,
            }),
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    assert!(
        acknowledged["acknowledged"]
            == json!(
                operations
                    .iter()
                    .map(|operation| operation.id)
                    .collect::<Vec<_>>()
            )
    );
    let after_edits = imported.snapshot(app, account).await;
    let (status, _) = account
        .request(app, "POST", &format!("{review}/confirm"), selection)
        .await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(provider.requests().len(), 3);
    assert!(imported.snapshot(app, account).await == after_edits);
    assert!(accepted(app, account, "GET", &associations, Value::Null).await == json!([]));
    assert!(accepted(app, account, "GET", &other_associations, Value::Null).await == json!([]));

    let fresh = accepted(
        app,
        account,
        "POST",
        &format!("{review}/context"),
        json!({"provider": selected}),
    )
    .await;
    let fresh_context: ArtistReviewContext = serde_json::from_value(fresh.clone()).unwrap();
    assert!(
        fresh_context.artist_operation_id.is_some()
            && fresh_context.artist_operation_id != captured.artist_operation_id
    );
    let mut expected = context;
    expected["artist_operation_id"] = json!(fresh_context.artist_operation_id);
    assert!(
        fresh == expected,
        "The restored visible artist must retain only its new accepted operation in the fresh context"
    );
    let selection = json!({"context": fresh, "provider_artist_id": catalog::SECOND_ARTIST});
    let preview = accepted(
        app,
        account,
        "POST",
        &format!("{review}/preview"),
        selection.clone(),
    )
    .await;
    assert!(preview["status"] == "available" && preview["context"] == fresh);
    assert!(preview["profile"] == json!(catalog::expected_profile()));
    let confirmed = accepted(
        app,
        account,
        "POST",
        &format!("{review}/confirm"),
        selection,
    )
    .await;
    assert!(
        confirmed["recording_id"] == imported.recording
            && confirmed["account_id"] == account.id
            && confirmed["provider_artist_id"] == catalog::SECOND_ARTIST
            && confirmed["evidence"] == "user_confirmed"
    );
    assert!(accepted(app, account, "GET", &associations, Value::Null).await == json!([confirmed]));
    assert!(accepted(app, account, "GET", &other_associations, Value::Null).await == json!([]));
    assert!(imported.snapshot(app, account).await == after_edits);
    assert_eq!(fixture::origin_counts(origin).await, origin_before);
    assert_eq!(provider.requests().len(), 5);
    assert_eq!(provider.unexpected(), 0);
    assert!(other.get(app, "/api/v1/plugins").await["installations"] == json!([]));
}

async fn accepted(app: &Router, account: &Account, method: &str, path: &str, body: Value) -> Value {
    let (status, headers, value) =
        fixture::exchange(app, method, path, body, account.headers()).await;
    assert_eq!(status, StatusCode::OK);
    assert!(
        headers
            .get("cache-control")
            .and_then(|value| value.to_str().ok())
            == Some("private, no-store")
    );
    value
}
