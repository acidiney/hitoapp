use crate::{
    album_editions::{Prepared, prepare},
    artist_review_fixture::{self as fixture, Account},
    support,
};
use axum::http::StatusCode;
use hito_core::test_support::album_edition_service::{self as provider, AlbumProvider};
use serde_json::{Value, json};
use uuid::Uuid;

#[tokio::test]
async fn an_account_compares_an_album_with_its_remembered_edition_only_when_it_asks() {
    let (_directory, app) = support::app().await;
    let account = Account::setup(&app).await;
    let mut server = AlbumProvider::start().await;
    let Prepared {
        release,
        first,
        association,
        ..
    } = prepare(&app, &account, &server).await;
    let path = format!("/api/v1/albums/{release}/edition/library-level");
    let body = json!({"release_group_id": provider::GROUP_ID});

    let mut anonymous = account.headers();
    anonymous.remove("cookie");
    let (status, _, _) = fixture::exchange(&app, "POST", &path, body.clone(), anonymous).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    let mut unguarded = account.headers();
    unguarded.remove("x-hito-request");
    let (status, _, _) = fixture::exchange(&app, "POST", &path, body.clone(), unguarded).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    for invalid in [
        json!({"release_group_id": ""}),
        json!({"release_group_id": "x".repeat(257)}),
    ] {
        let (status, _, _) =
            fixture::exchange(&app, "POST", &path, invalid, account.headers()).await;
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    }
    let mut unknown = body.clone();
    unknown["extra"] = json!(true);
    let (status, _, _) = fixture::exchange(&app, "POST", &path, unknown, account.headers()).await;
    assert!(status.is_client_error());
    let (status, _, _) = fixture::exchange(
        &app,
        "POST",
        &format!("/api/v1/albums/{}/edition/library-level", Uuid::new_v4()),
        body.clone(),
        account.headers(),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    let requests = server.requests().len();
    let (status, headers, needs) =
        fixture::exchange(&app, "POST", &path, body.clone(), account.headers()).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(headers.get("cache-control").unwrap(), "private, no-store");
    assert_eq!(needs, json!({"status": "needs_choice"}));
    assert_eq!(
        server.requests().len(),
        requests,
        "no edition is fetched without a remembered edition"
    );

    let (status, _, _) = fixture::exchange(
        &app,
        "PUT",
        &format!("/api/v1/albums/{release}/edition"),
        json!({
            "request_id": Uuid::new_v4().to_string(), "expected_revision": 0,
            "recording_id": first, "association_id": association,
            "release_group_id": provider::GROUP_ID, "edition_id": provider::EDITION_ID
        }),
        account.headers(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let (status, _, compared) =
        fixture::exchange(&app, "POST", &path, body.clone(), account.headers()).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        compared,
        json!({"status": "compared", "level": "partly", "matched_tracks": 3, "edition_tracks": 6})
    );

    server.set_outage(true);
    let (status, _, outage) = fixture::exchange(&app, "POST", &path, body, account.headers()).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(outage, json!({"status": "unavailable"}));
    assert_eq!(server.unexpected(), 0);

    let contract: Value = serde_json::from_str(include_str!("../../../../docs/openapi.json"))
        .expect("the committed contract must parse");
    let operation = &contract["paths"]["/api/v1/albums/{id}/edition/library-level"]["post"];
    assert_eq!(operation["operationId"], "album_library_level");
    assert_eq!(
        operation["responses"]["200"]["content"]["application/json"]["schema"]["$ref"],
        "#/components/schemas/AlbumLibraryLevel"
    );
    assert!(server.finish().await);
}
