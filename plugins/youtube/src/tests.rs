use crate::{Config, router};
use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use hito_plugin_sdk::SearchPage;
use http_body_util::BodyExt;
use std::{fs, os::unix::fs::PermissionsExt};
use tower::ServiceExt;

mod artwork;
mod cancellation;
mod credits;
mod failures;
mod health;
#[cfg(target_os = "linux")]
mod lifecycle;
mod logging;
mod manifest;
mod music_search;
mod sources;

#[tokio::test]
async fn search_uses_an_owned_executable_and_returns_bounded_sdk_recordings() {
    let directory = tempfile::tempdir().unwrap();
    let executable = directory.path().join("owned-yt-dlp");
    fs::write(&executable, include_str!("fixtures/search.sh")).unwrap();
    fs::set_permissions(&executable, fs::Permissions::from_mode(0o700)).unwrap();
    let marker = directory.path().join("shell-executed");
    let query = format!(
        "Owned tune; $(touch {}) `printf unexpected` 'literal' \"query\"",
        marker.display()
    );
    let mut expected_query = url::Url::parse("https://music.youtube.com/search").unwrap();
    expected_query.query_pairs_mut().append_pair("q", &query);
    expected_query.set_fragment(Some("songs"));
    let app = router(Config { executable });
    let mut identities = Vec::new();

    for _ in 0..2 {
        let response = app
            .clone()
            .oneshot(
                Request::post("/v1/search")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        serde_json::json!({"query":query,"cursor":null,"limit":2}).to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let bytes = response.into_body().collect().await.unwrap().to_bytes();
        assert!(bytes.len() <= 262_144);
        let body = std::str::from_utf8(&bytes).unwrap();
        assert!(!body.contains("https://"));
        assert!(!body.contains("private-upstream-token"));
        let page: SearchPage = serde_json::from_slice(&bytes).unwrap();
        page.validate(2).unwrap();
        assert_eq!(page.items.len(), 2);
        assert!(page.next_cursor.is_none());
        assert_eq!(page.items[0].title, "Owned tone one");
        assert_eq!(page.items[0].artist, "Hito test performer");
        assert_eq!(page.items[0].duration_ms, Some(12_500));
        for item in &page.items {
            assert_eq!(item.id.len(), 11);
            assert!(
                item.id
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || b"_-".contains(&byte))
            );
        }
        identities.push(
            page.items
                .into_iter()
                .map(|item| item.id)
                .collect::<Vec<_>>(),
        );
    }

    assert_eq!(identities[0], ["HitoTone001", "HitoTone002"]);
    assert_eq!(identities[1], identities[0]);
    assert!(!marker.exists());
    let observed = fs::read_to_string(directory.path().join("argv")).unwrap();
    let invocations: Vec<_> = observed.split("invocation\n").skip(1).collect();
    assert_eq!(invocations.len(), 2);
    for invocation in invocations {
        let arguments: Vec<_> = invocation.lines().collect();
        for flag in [
            "--ignore-config",
            "--no-config-locations",
            "--simulate",
            "--skip-download",
            "--no-cookies",
            "--no-cookies-from-browser",
            "--no-plugin-dirs",
            "--no-remote-components",
            "--dump-single-json",
        ] {
            assert!(arguments.contains(&flag), "missing process policy {flag}");
        }
        assert!(
            arguments.last().copied() == Some(expected_query.as_str()),
            "search must preserve the encoded Music query as one literal URL argument"
        );
        assert_eq!(arguments[arguments.len() - 2], "--");
        let end = arguments
            .iter()
            .position(|argument| *argument == "--playlist-end")
            .unwrap_or_else(|| panic!("search must set its extraction result bound"));
        assert!(
            end + 2 == arguments.len() - 2 && arguments[end + 1] == "2",
            "search must pass the requested result bound before the URL separator"
        );
        assert_eq!(
            arguments
                .iter()
                .filter(|argument| **argument == expected_query.as_str())
                .count(),
            1
        );
    }
}
