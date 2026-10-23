use crate::{Config, Service};
use axum::{
    body::Body,
    http::{Request, StatusCode},
    response::Response,
};
use futures_util::FutureExt;
use hito_plugin_sdk::{ProviderRecording, RecordingMetadata, SearchPage, SourcePage};
use http_body_util::BodyExt;
use serde_json::{Value, json};
use std::{fs, os::unix::fs::PermissionsExt, panic::AssertUnwindSafe, time::Duration};
use tower::ServiceExt;
use url::Url;

#[tokio::test]
async fn music_search_preserves_flat_ranked_results_until_independent_metadata_hydration() {
    let directory = tempfile::tempdir().unwrap();
    let executable = directory.path().join("owned-yt-dlp");
    fs::write(&executable, include_str!("../fixtures/music-search.sh")).unwrap();
    fs::set_permissions(&executable, fs::Permissions::from_mode(0o700)).unwrap();
    fs::write(
        directory.path().join("search.json"),
        json!({"entries": [
            {"id": "zzzzzzzzzzz", "title": "Owned first music result"},
            {"id": "aaaaaaaaaaa", "title": "Owned second music result"},
            {"id": "bbbbbbbbbbb", "title": "Owned extra music result"}
        ]})
        .to_string(),
    )
    .unwrap();
    fs::write(directory.path().join("recording.json"), json!({
        "id": "zzzzzzzzzzz", "title": "Owned performer - Owned tune (Official Audio)",
        "artists": ["Owned performer"], "track": "Owned tune", "album": "Owned album",
        "duration": 12.5, "format_id": "140", "ext": "m4a", "acodec": "mp4a.40.2",
        "vcodec": "none", "protocol": "https", "filesize": 4096,
        "url": "https://owned.googlevideo.com/videoplayback?expire=4102444800&lmt=1730000000000000&token=private-owned-token",
        "is_live": false, "live_status": "not_live"
    }).to_string()).unwrap();
    let marker = directory.path().join("shell-executed");
    let query = format!(
        "Owned música & + # ?; $(touch {}) `printf unexpected` 'literal' \"query\"",
        marker.display()
    );
    let mut expected = Url::parse("https://music.youtube.com/search").unwrap();
    expected.query_pairs_mut().append_pair("q", &query);
    expected.set_fragment(Some("songs"));
    let service = Service::new(Config { executable });
    let result = tokio::time::timeout(Duration::from_secs(8), AssertUnwindSafe(async {
        let response = post(&service, "/v1/search", json!({"query": query, "cursor": null, "limit": 2})).await;
        let arguments = fs::read(directory.path().join("last-argv")).unwrap();
        let arguments = arguments.split(|byte| *byte == 0).filter(|argument| !argument.is_empty())
            .map(|argument| std::str::from_utf8(argument).unwrap()).collect::<Vec<_>>();
        let separator = arguments.iter().position(|argument| *argument == "--")
            .unwrap_or_else(|| panic!("the extractor must fence the URL after its options"));
        assert!(separator + 2 == arguments.len() && arguments[separator + 1] == expected.as_str(),
            "search must use the encoded YouTube Music songs URL as one literal argument");
        let end = arguments.iter().position(|argument| *argument == "--playlist-end")
            .unwrap_or_else(|| panic!("music search must bound the extracted playlist before its URL"));
        assert!(end + 1 < separator && arguments[end + 1] == "2" &&
            arguments.iter().filter(|argument| **argument == "--playlist-end").count() == 1,
            "the exact requested result limit must precede the option separator");
        assert!(!marker.exists(), "search query text must never execute through a shell");
        assert!(response.status() == StatusCode::OK, "flat music search must return SDK summaries");
        let bytes = response.into_body().collect().await.unwrap().to_bytes();
        let page: SearchPage = serde_json::from_slice(&bytes).unwrap();
        assert!(page.validate(2).is_ok() && page.items.len() == 2 && page.next_cursor.is_none() &&
            page.items[0].id == "zzzzzzzzzzz" && page.items[1].id == "aaaaaaaaaaa" &&
            page.items[0].title == "Owned first music result" && page.items[1].title == "Owned second music result" &&
            page.items.iter().all(|item| item.artist == "Unknown artist" && item.duration_ms.is_none()),
            "id/title-only music results must retain provider order without invented query metadata");

        let payload = json!({"recording_id": page.items[0].id});
        let response = post(&service, "/v1/recordings/detail", payload.clone()).await;
        assert!(response.status() == StatusCode::OK, "the first music result must hydrate independently");
        let detail: ProviderRecording = serde_json::from_slice(&response.into_body().collect().await.unwrap().to_bytes()).unwrap();
        let response = post(&service, "/v1/sources", payload.clone()).await;
        assert!(response.status() == StatusCode::OK, "hydration must identify an actual audio witness");
        let sources: SourcePage = serde_json::from_slice(&response.into_body().collect().await.unwrap().to_bytes()).unwrap();
        let response = post(&service, "/v1/recordings/metadata", payload).await;
        assert!(response.status() == StatusCode::OK, "hydration must supply independently extracted musical metadata");
        let metadata: RecordingMetadata = serde_json::from_slice(&response.into_body().collect().await.unwrap().to_bytes()).unwrap();
        assert!(detail.validate().is_ok() && metadata.validate().is_ok() && sources.validate().is_ok() &&
            detail.id == page.items[0].id && detail.artist == "Owned performer" && detail.duration_ms == Some(12_500) &&
            detail.title == "Owned performer - Owned tune (Official Audio)" && metadata.recording == detail &&
            metadata.musical_title.as_deref() == Some("Owned tune") && metadata.album_title.as_deref() == Some("Owned album") &&
            sources.items.len() == 1 && sources.items[0].ready &&
            metadata.source_ids == Some(vec![sources.items[0].id.clone()]),
            "only hydrated evidence may provide musical identity and complete ready source witnesses");
        assert!(fs::read_to_string(directory.path().join("calls")).unwrap().lines().count() == 2,
            "detail, sources and metadata must share one bounded extraction after the search");
        for malformed in [
            json!({"entries": [{"id": "VLPLowned-playlist", "title": "Owned playlist"}]}),
            json!({"entries": [{"id": "bad/video01", "title": "Owned invalid video"}]}),
            json!({"entries": "not a result list"}),
        ] {
            fs::write(directory.path().join("search.json"), malformed.to_string()).unwrap();
            let response = post(&service, "/v1/search", json!({"query": query, "cursor": null, "limit": 2})).await;
            assert!(response.status() == StatusCode::BAD_GATEWAY,
                "music search must reject playlist identities, invalid video IDs and malformed result lists");
            let body = response.into_body().collect().await.unwrap().to_bytes();
            assert!(!body.windows(b"private-".len()).any(|part| part == b"private-"),
                "rejected music data must not expose private extractor material");
        }
        assert!(!marker.exists(), "no search invocation may evaluate query shell syntax");
    }).catch_unwind()).await;
    let cleanup = tokio::time::timeout(Duration::from_secs(3), service.shutdown()).await;
    assert!(
        matches!(cleanup, Ok(Ok(()))),
        "the owned music extractor must shut down and join"
    );
    match result {
        Ok(Ok(())) => {}
        Ok(Err(panic)) => std::panic::resume_unwind(panic),
        Err(_) => panic!("the public music search exercise must settle within its owned deadline"),
    }
}

async fn post(service: &Service, path: &str, payload: Value) -> Response {
    service
        .router()
        .oneshot(
            Request::post(path)
                .header("content-type", "application/json")
                .body(Body::from(payload.to_string()))
                .unwrap(),
        )
        .await
        .unwrap()
}
