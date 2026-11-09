use crate::{Config, Service};
use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use hito_plugin_sdk::{CreditRole, ProviderRecording, SearchPage};
use http_body_util::BodyExt;
use serde_json::{Value, json};
use std::{fs, os::unix::fs::PermissionsExt};
use tower::ServiceExt;

fn recording(artists: Value) -> Value {
    json!({
        "id": "zzzzzzzzzzz", "title": "Owned tune", "artists": artists,
        "creators": ["Owned channel owner"], "channel": "Owned channel",
        "duration": 12.5, "format_id": "140", "ext": "m4a", "acodec": "mp4a.40.2",
        "vcodec": "none", "protocol": "https", "filesize": 4096,
        "url": "https://owned.googlevideo.com/videoplayback?expire=4102444800&lmt=1730000000000000&token=private-owned-token",
        "is_live": false, "live_status": "not_live"
    })
}

async fn post(service: &Service, path: &str, payload: Value) -> (StatusCode, Vec<u8>) {
    let response = service
        .router()
        .oneshot(
            Request::post(path)
                .header("content-type", "application/json")
                .body(Body::from(payload.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    let status = response.status();
    (
        status,
        response
            .into_body()
            .collect()
            .await
            .unwrap()
            .to_bytes()
            .to_vec(),
    )
}

async fn detail(artists: Value) -> ProviderRecording {
    let directory = tempfile::tempdir().unwrap();
    let executable = directory.path().join("owned-yt-dlp");
    fs::write(&executable, include_str!("../fixtures/music-search.sh")).unwrap();
    fs::set_permissions(&executable, fs::Permissions::from_mode(0o700)).unwrap();
    fs::write(
        directory.path().join("recording.json"),
        recording(artists).to_string(),
    )
    .unwrap();
    let service = Service::new(Config { executable });
    let (status, bytes) = post(
        &service,
        "/v1/recordings/detail",
        json!({"recording_id": "zzzzzzzzzzz"}),
    )
    .await;
    service.shutdown().await.unwrap();
    assert_eq!(status, StatusCode::OK);
    let detail: ProviderRecording = serde_json::from_slice(&bytes).unwrap();
    detail.validate().unwrap();
    detail
}

#[tokio::test]
async fn the_music_artists_list_becomes_ordered_main_credits_without_guessing_roles() {
    let detail = detail(json!(["Owned performer", "Owned guest"])).await;
    assert_eq!(detail.artist, "Owned performer, Owned guest");
    let credited: Vec<_> = detail
        .artists
        .iter()
        .map(|artist| (artist.name.as_str(), artist.role, artist.join.as_deref()))
        .collect();
    assert_eq!(
        credited,
        [
            ("Owned performer", CreditRole::Main, Some(", ")),
            ("Owned guest", CreditRole::Main, None)
        ]
    );
    assert!(
        detail
            .artists
            .iter()
            .all(|artist| { artist.provider_artist_id.is_none() && artist.page_url.is_none() })
    );
}

#[tokio::test]
async fn creators_and_channels_are_never_credited_as_artists() {
    let detail = detail(Value::Null).await;
    assert!(detail.artists.is_empty());
    assert_eq!(detail.artist, "Channel: Owned channel");
}

#[tokio::test]
async fn a_list_that_cannot_be_credited_whole_leaves_the_display_text_alone() {
    let repeated = detail(json!(["Owned performer", " owned PERFORMER "])).await;
    assert!(repeated.artists.is_empty());
    assert_eq!(repeated.artist, "Owned performer,  owned PERFORMER ");
    let long = detail(json!(["é".repeat(129)])).await;
    assert!(long.artists.is_empty());
}

#[tokio::test]
async fn music_search_results_carry_their_artist_list_too() {
    let directory = tempfile::tempdir().unwrap();
    let executable = directory.path().join("owned-yt-dlp");
    fs::write(&executable, include_str!("../fixtures/music-search.sh")).unwrap();
    fs::set_permissions(&executable, fs::Permissions::from_mode(0o700)).unwrap();
    fs::write(
        directory.path().join("search.json"),
        json!({"entries": [{"id": "zzzzzzzzzzy", "title": "Owned tune", "artists": ["Ada", "Bo"]}]})
            .to_string(),
    )
    .unwrap();
    let service = Service::new(Config { executable });
    let (status, bytes) = post(
        &service,
        "/v1/search",
        json!({"query": "owned", "cursor": null, "limit": 1}),
    )
    .await;
    service.shutdown().await.unwrap();
    assert_eq!(status, StatusCode::OK);
    let page: SearchPage = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(page.items[0].artists.len(), 2);
    assert_eq!(page.items[0].artists[1].name, "Bo");
}
