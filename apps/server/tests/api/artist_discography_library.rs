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
async fn an_account_sees_which_release_groups_are_linked_to_its_albums_without_provider_requests() {
    let (_directory, app) = support::app().await;
    let account = Account::setup(&app).await;
    let mut server = AlbumProvider::start().await;
    let Prepared {
        release,
        first,
        association,
        ..
    } = prepare(&app, &account, &server).await;
    let path =
        format!("/api/v1/recordings/{first}/artist-associations/{association}/discography/library");
    let body = json!({"release_group_ids": [provider::GROUP_ID, "group:other"]});
    let requests = server.requests().len();

    let mut anonymous = account.headers();
    anonymous.remove("cookie");
    let (status, _, _) = fixture::exchange(&app, "POST", &path, body.clone(), anonymous).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    let mut unguarded = account.headers();
    unguarded.remove("x-hito-request");
    let (status, _, _) = fixture::exchange(&app, "POST", &path, body.clone(), unguarded).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    let mut unknown = body.clone();
    unknown["extra"] = json!(true);
    let (status, _, _) = fixture::exchange(&app, "POST", &path, unknown, account.headers()).await;
    assert!(status.is_client_error());
    let (status, _, _) = fixture::exchange(
        &app,
        "POST",
        &path,
        json!({"release_group_ids": []}),
        account.headers(),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    let (status, _, _) = fixture::exchange(
        &app,
        "POST",
        &format!(
            "/api/v1/recordings/{first}/artist-associations/{}/discography/library",
            Uuid::new_v4()
        ),
        body.clone(),
        account.headers(),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    let (status, headers, unlinked) =
        fixture::exchange(&app, "POST", &path, body.clone(), account.headers()).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(headers["cache-control"], "private, no-store");
    assert_eq!(
        unlinked,
        json!({"association_id": association, "groups": [
            {"release_group_id": provider::GROUP_ID, "albums": []},
            {"release_group_id": "group:other", "albums": []},
        ]})
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
    let (status, _, linked): (_, _, Value) =
        fixture::exchange(&app, "POST", &path, body, account.headers()).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        linked["groups"][0]["albums"],
        json!([{
            "release_id": release, "edition_id": provider::EDITION_ID,
            "track_count": 3, "playable_count": 0
        }])
    );
    assert_eq!(linked["groups"][1]["albums"], json!([]));
    assert_eq!(
        server.requests().len(),
        requests,
        "The library join never contacts a provider"
    );
    let (status, _) = account
        .request(
            &app,
            "PUT",
            "/api/v1/account/pin",
            json!({"password": "owned artist review administrator password", "pin": "1234", "idle_minutes": 0}),
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    let (status, _) = account
        .request(&app, "POST", "/api/v1/account/lock", Value::Null)
        .await;
    assert_eq!(status, StatusCode::OK);
    let (status, _, locked) = fixture::exchange(
        &app,
        "POST",
        &path,
        json!({"release_group_ids": [provider::GROUP_ID]}),
        account.headers(),
    )
    .await;
    assert_eq!(status, StatusCode::LOCKED);
    assert_eq!(locked["code"], "locked");
    assert!(server.finish().await);
}
