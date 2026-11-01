use super::support::{admin, call};
use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use hito_core::test_support::{
    musicbrainz::{FakeMusicBrainz, Reply},
    playlist_service as provider,
    support::write_wav,
};
use http_body_util::BodyExt;
use lofty::{
    prelude::{Accessor, ItemKey, TagExt},
    tag::{Tag, TagType},
};
use serde_json::{Value, json};
use tower::ServiceExt;
use uuid::Uuid;

const ISRC: &str = "USRC17607839";

pub(super) async fn import(
    app: &axum::Router,
    token: &str,
    entries: Vec<Value>,
) -> (provider::ProviderServer, Vec<String>) {
    let token = Some(token);
    let origin = provider::ProviderServer::start().await;
    origin
        .reply(
            "v1/manifest",
            json!({"id": "org.hito.isrc-fixture", "name": "ISRC fixture",
                "protocol_version": 1, "capabilities": ["playlists"]}),
        )
        .await;
    provider::playlist(&origin, "isrc-list", "isrc-v1", entries).await;
    let (_, service) = call(
        app,
        "POST",
        "/api/v1/plugins/services",
        json!({"name": "ISRC fixture", "base_url": origin.url}),
        token,
    )
    .await;
    let (_, installation) = call(
        app,
        "POST",
        "/api/v1/plugins",
        json!({"service_id": service["id"], "service_revision": service["revision"]}),
        token,
    )
    .await;
    let (status, receipt) = call(
        app,
        "POST",
        &format!(
            "/api/v1/plugins/{}/playlists/import",
            installation["id"].as_str().unwrap()
        ),
        json!({
            "request_id": Uuid::new_v4(),
            "installation_revision": installation["revision"],
            "playlist_reference": "https://provider.invalid/isrc-list"
        }),
        token,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let (_, detail) = call(
        app,
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
    (origin, recordings)
}

async fn page(app: &axum::Router, path: &str, token: &str) -> Value {
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .uri(path)
                .header("x-inertia", "true")
                .header("authorization", format!("Bearer {token}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    serde_json::from_slice(&response.into_body().collect().await.unwrap().to_bytes()).unwrap()
}

#[tokio::test]
async fn a_local_file_and_an_imported_track_with_one_isrc_are_one_card_until_split() {
    let musicbrainz = FakeMusicBrainz::start().await;
    musicbrainz.isrc(ISRC, Reply::NotFound);
    let dir = tempfile::tempdir().unwrap();
    let music = dir.path().join("music");
    std::fs::create_dir(&music).unwrap();
    let path = music.join("same.wav");
    write_wav(&path, 1);
    let mut tag = Tag::new(TagType::Id3v2);
    tag.set_title("Same song".into());
    tag.set_artist("C4 Pedro".into());
    tag.insert_text(ItemKey::Isrc, ISRC.into());
    tag.save_to_path(&path, Default::default()).unwrap();
    let mut config = hito_core::CoreConfig::new(
        dir.path().join("hito.db"),
        music,
        hito_core::HostDeviceId::new("api-test-host").unwrap(),
    );
    config.musicbrainz_loopback_endpoint = Some(musicbrainz.endpoint.clone());
    let hito = hito_core::open(config).await.unwrap();
    let app = hito_server::router(hito.clone());
    let session = admin(&app).await;
    let token = Some(session.as_str());
    let (_, libraries) = call(&app, "GET", "/api/v1/libraries", Value::Null, token).await;
    let library = libraries[0]["id"].as_str().unwrap().to_owned();
    call(
        &app,
        "POST",
        &format!("/api/v1/libraries/{library}/scan"),
        Value::Null,
        token,
    )
    .await;
    hito.storage.run_next_job().await.unwrap();
    let entry = json!({"occurrence_id": "only", "item": {"kind": "recording", "recording": {
        "id": "imported", "title": "Same song", "artist": "C4 Pedro", "duration_ms": 1000,
        "release": null, "disc_number": null, "track_number": null, "revision": "v1",
        "isrc": "us-rc1-76-07839"
    }}});
    let (_origin, imported) = import(&app, &session, vec![entry]).await;
    let (_, before) = call(&app, "GET", "/api/v1/recordings", Value::Null, token).await;
    assert_eq!(before["total"], 2, "two cards until the walk has proof");

    while hito
        .artist_catalog
        .run_next_automatic_matching_until(std::future::pending())
        .await
        .unwrap()
    {}
    let (_, after) = call(&app, "GET", "/api/v1/recordings", Value::Null, token).await;
    assert_eq!(after["total"], 1, "the proven pair is one card");
    let card = after["items"][0]["id"].as_str().unwrap().to_owned();
    assert_ne!(
        card, imported[0],
        "the playable local file carries the card"
    );
    let (status, copies) = call(
        &app,
        "GET",
        &format!("/api/v1/recordings/{card}/copies"),
        Value::Null,
        token,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(copies["canonical_id"], card);
    let listed = copies["copies"].as_array().unwrap();
    assert_eq!(listed.len(), 2);
    let member = listed
        .iter()
        .find(|copy| copy["canonical"] == false)
        .unwrap();
    assert_eq!(member["recording_id"], imported[0]);
    assert_eq!(member["proof"], "isrc");
    assert!(!copies.to_string().contains(dir.path().to_str().unwrap()));
    let details = page(&app, &format!("/recordings/{card}"), &session).await;
    assert_eq!(details["component"], "RecordingDetailsPage");
    assert_eq!(details["props"]["copies"]["copies"], copies["copies"]);
    let (status, _) = call(
        &app,
        "GET",
        &format!("/api/v1/recordings/{card}/copies"),
        Value::Null,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    let (status, _) = call(
        &app,
        "GET",
        "/api/v1/recordings/not-a-recording/copies",
        Value::Null,
        token,
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    let split_path = format!("/api/v1/recordings/{}/copies/split", imported[0]);
    for _ in 0..2 {
        let (status, split) = call(&app, "POST", &split_path, Value::Null, token).await;
        assert_eq!((status, &split["separate"]), (StatusCode::OK, &json!(true)));
    }
    let (_, separate) = call(&app, "GET", "/api/v1/recordings", Value::Null, token).await;
    assert_eq!(separate["total"], 2);
    let (status, _) = call(
        &app,
        "POST",
        &format!("/api/v1/recordings/{card}/copies/split"),
        Value::Null,
        token,
    )
    .await;
    assert_eq!(
        status,
        StatusCode::OK,
        "a recording that carries no group is already separate"
    );
}
