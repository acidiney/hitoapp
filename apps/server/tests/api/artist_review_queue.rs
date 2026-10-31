use super::support::{admin, call};
use axum::http::StatusCode;
use hito_core::test_support::{
    musicbrainz::{FakeMusicBrainz, Reply},
    playlist_service as provider,
};
use serde_json::{Value, json};
use uuid::Uuid;

const NOVA: &str = "44444444-4444-4444-8444-444444444444";
const NAMESAKE: &str = "55555555-5555-4555-8555-555555555555";
const QUEUE: &str = "/api/v1/artist-matching/review-queue";

async fn queued() -> (
    tempfile::TempDir,
    axum::Router,
    String,
    FakeMusicBrainz,
    provider::ProviderServer,
    Vec<String>,
) {
    let musicbrainz = FakeMusicBrainz::start().await;
    musicbrainz.search(
        "Nova Quartet",
        Reply::Candidates(vec![
            (NOVA.into(), "Nova Quartet".into()),
            (NAMESAKE.into(), "Nova Quartet".into()),
        ]),
    );
    musicbrainz.describe(NOVA, "Lisbon fado band", "PT");
    musicbrainz.recordings_of(NOVA, Reply::Discography(vec![]));
    musicbrainz.recordings_of(NAMESAKE, Reply::Discography(vec![]));
    let dir = tempfile::tempdir().unwrap();
    let music = dir.path().join("music");
    std::fs::create_dir(&music).unwrap();
    let mut config = hito_core::CoreConfig::new(
        dir.path().join("hito.db"),
        music,
        hito_core::HostDeviceId::new("api-test-host").unwrap(),
    );
    config.musicbrainz_loopback_endpoint = Some(musicbrainz.endpoint.clone());
    let hito = hito_core::open(config).await.unwrap();
    let app = hito_server::router(hito);
    let session = admin(&app).await;
    let token = Some(session.as_str());
    let entry = |id: &str, title: &str, length: u64| {
        json!({"occurrence_id": id, "item": {"kind": "recording", "recording": {
            "id": id, "title": title, "artist": "Nova Quartet", "duration_ms": length,
            "release": null, "disc_number": null, "track_number": null, "revision": "v1"
        }}})
    };
    let origin = provider::ProviderServer::start().await;
    origin
        .reply(
            "v1/manifest",
            json!({"id": "org.hito.review-fixture", "name": "Review fixture",
                "protocol_version": 1, "capabilities": ["playlists"]}),
        )
        .await;
    provider::playlist(
        &origin,
        "review-list",
        "review-v1",
        vec![
            entry("one", "First Song", 100_000),
            entry("two", "Second Song", 120_000),
            entry("three", "Third Song", 140_000),
        ],
    )
    .await;
    let (_, service) = call(
        &app,
        "POST",
        "/api/v1/plugins/services",
        json!({"name": "Review fixture", "base_url": origin.url}),
        token,
    )
    .await;
    let (_, installation) = call(
        &app,
        "POST",
        "/api/v1/plugins",
        json!({"service_id": service["id"], "service_revision": service["revision"]}),
        token,
    )
    .await;
    let (_, receipt) = call(
        &app,
        "POST",
        &format!(
            "/api/v1/plugins/{}/playlists/import",
            installation["id"].as_str().unwrap()
        ),
        json!({
            "request_id": Uuid::new_v4(),
            "installation_revision": installation["revision"],
            "playlist_reference": "https://provider.invalid/review-list"
        }),
        token,
    )
    .await;
    let (_, detail) = call(
        &app,
        "GET",
        &format!(
            "/api/v1/playlists/{}",
            receipt["playlist_id"].as_str().unwrap()
        ),
        Value::Null,
        token,
    )
    .await;
    let recordings = detail["entries"]
        .as_array()
        .unwrap()
        .iter()
        .map(|entry| entry["recording_id"].as_str().unwrap().to_owned())
        .collect();
    let path = "/api/v1/artist-matching/provider-evidence";
    let (status, names) = call(
        &app,
        "POST",
        path,
        json!({"request_id": Uuid::new_v4(), "after": "name:"}),
        token,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        (&names["matched"], &names["uncertain"]),
        (&json!(0), &json!(3))
    );
    (dir, app, session, musicbrainz, origin, recordings)
}

#[tokio::test]
async fn the_review_queue_lists_confirms_and_dismisses_through_the_api() {
    let (_dir, app, session, musicbrainz, _origin, recordings) = queued().await;
    let token = Some(session.as_str());
    let asked = musicbrainz.requests().len();
    let (status, page) = call(&app, "GET", QUEUE, Value::Null, token).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        (&page["total"], &page["limit"], &page["offset"]),
        (&json!(1), &json!(20), &json!(0))
    );
    let item = &page["items"][0];
    assert_eq!(
        (&item["name"], &item["undecided_tracks"]),
        (&json!("Nova Quartet"), &json!(3))
    );
    let first = &item["candidates"][0];
    assert_eq!(first["provider_artist_id"], NOVA);
    assert_eq!(first["disambiguation"], "Lisbon fado band");
    assert_eq!(first["country"], "PT");
    assert_eq!(first["kind"], "person");
    assert_eq!(item["candidates"][1]["provider_artist_id"], NAMESAKE);
    assert_eq!(
        musicbrainz.requests().len(),
        asked,
        "Reading asks MusicBrainz nothing"
    );
    for query in ["?limit=0", "?limit=51", "?offset=-1"] {
        assert_eq!(
            call(&app, "GET", &format!("{QUEUE}{query}"), Value::Null, token)
                .await
                .0,
            StatusCode::UNPROCESSABLE_ENTITY,
            "{query}"
        );
    }

    let credit = item["artist_id"].as_str().unwrap();
    let confirm = format!("{QUEUE}/{credit}/confirm");
    let dismiss = format!("{QUEUE}/{credit}/dismiss");
    for bad in [
        json!({"request_id": Uuid::nil(), "provider_artist_id": NOVA}),
        json!({"request_id": Uuid::new_v4()}),
        json!({"request_id": Uuid::new_v4(), "provider_artist_id": NOVA, "extra": 1}),
    ] {
        assert_eq!(
            call(&app, "POST", &confirm, bad, token).await.0,
            StatusCode::UNPROCESSABLE_ENTITY
        );
    }
    assert_eq!(
        call(
            &app,
            "POST",
            &confirm,
            json!({"request_id": Uuid::new_v4(), "provider_artist_id": "66666666-6666-4666-8666-666666666666"}),
            token
        )
        .await
        .0,
        StatusCode::NOT_FOUND
    );

    let outsider = {
        call(
            &app,
            "POST",
            "/api/v1/users",
            json!({"username": "outsider", "password": "owned review outsider password"}),
            token,
        )
        .await;
        let (_, login) = call(
            &app,
            "POST",
            "/api/v1/login",
            json!({"username": "outsider", "password": "owned review outsider password"}),
            None,
        )
        .await;
        login["token"].as_str().unwrap().to_owned()
    };
    let outsider = Some(outsider.as_str());
    let (_, empty) = call(&app, "GET", QUEUE, Value::Null, outsider).await;
    assert_eq!(empty["total"], 0, "Another account has its own queue");
    for (path, body) in [
        (
            &confirm,
            json!({"request_id": Uuid::new_v4(), "provider_artist_id": NOVA}),
        ),
        (&dismiss, json!({})),
    ] {
        assert_eq!(
            call(&app, "POST", path, body, outsider).await.0,
            StatusCode::NOT_FOUND
        );
    }
    assert_eq!(
        call(&app, "GET", QUEUE, Value::Null, None).await.0,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        call(&app, "POST", &dismiss, json!({}), None).await.0,
        StatusCode::UNAUTHORIZED
    );

    let body = json!({"request_id": Uuid::new_v4(), "provider_artist_id": NOVA});
    let (status, receipt) = call(&app, "POST", &confirm, body.clone(), token).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        (&receipt["applied"], &receipt["kept"], &receipt["remaining"]),
        (&json!(3), &json!(0), &json!(0))
    );
    assert_eq!(
        call(&app, "POST", &confirm, body, token).await,
        (StatusCode::OK, receipt),
        "An exact retry replays the receipt"
    );
    let (_, associations) = call(
        &app,
        "GET",
        &format!("/api/v1/recordings/{}/artist-associations", recordings[0]),
        Value::Null,
        token,
    )
    .await;
    assert_eq!(associations[0]["evidence"], "user_confirmed_candidate");
    assert_eq!(associations[0]["provider_artist_id"], NOVA);
    let (_, after) = call(&app, "GET", QUEUE, Value::Null, token).await;
    assert_eq!(after["total"], 0);
    let (_, groups) = call(
        &app,
        "GET",
        "/api/v1/artist-identity-groups",
        Value::Null,
        token,
    )
    .await;
    assert_eq!(groups["items"][0]["provider_artist_id"], NOVA);
}

#[tokio::test]
async fn none_of_these_leaves_the_queue_and_a_locked_session_is_refused() {
    let (_dir, app, session, _musicbrainz, _origin, _) = queued().await;
    let token = Some(session.as_str());
    let (_, page) = call(&app, "GET", QUEUE, Value::Null, token).await;
    let credit = page["items"][0]["artist_id"].as_str().unwrap().to_owned();
    let dismiss = format!("{QUEUE}/{credit}/dismiss");
    assert_eq!(
        call(&app, "POST", &dismiss, json!({"extra": 1}), token)
            .await
            .0,
        StatusCode::UNPROCESSABLE_ENTITY
    );
    assert_eq!(
        call(
            &app,
            "POST",
            &dismiss,
            json!({"provider_artist_id": "66666666-6666-4666-8666-666666666666"}),
            token
        )
        .await
        .0,
        StatusCode::NOT_FOUND
    );
    let (status, one) = call(
        &app,
        "POST",
        &dismiss,
        json!({"provider_artist_id": NOVA}),
        token,
    )
    .await;
    assert_eq!((status, &one["dismissed"]), (StatusCode::OK, &json!(1)));
    let (_, all) = call(&app, "POST", &dismiss, json!({}), token).await;
    assert_eq!(all["dismissed"], 1);
    let (_, after) = call(&app, "GET", QUEUE, Value::Null, token).await;
    assert_eq!(after["total"], 0);

    assert_eq!(
        call(
            &app,
            "PUT",
            "/api/v1/account/pin",
            json!({"password": "administrator test password", "pin": "1234", "idle_minutes": 0}),
            token,
        )
        .await
        .0,
        StatusCode::OK
    );
    call(&app, "POST", "/api/v1/account/lock", Value::Null, token).await;
    let confirm = format!("{QUEUE}/{credit}/confirm");
    for (method, path, body) in [
        ("GET", QUEUE.to_owned(), Value::Null),
        (
            "POST",
            confirm,
            json!({"request_id": Uuid::new_v4(), "provider_artist_id": NOVA}),
        ),
        ("POST", dismiss, json!({})),
    ] {
        assert_eq!(
            call(&app, method, &path, body, token).await.0,
            StatusCode::LOCKED,
            "{path}"
        );
    }
}
