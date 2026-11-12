use crate::{Config, Service};
use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use futures_util::FutureExt;
use hito_plugin_sdk::SearchPage;
use http_body_util::BodyExt;
use serde_json::{Value, json};
use std::{
    fs,
    future::Future,
    os::unix::fs::PermissionsExt,
    panic::AssertUnwindSafe,
    path::{Path, PathBuf},
    time::{Duration, Instant},
};
use tower::ServiceExt;
use url::Url;

const QUERY: &str = "Owned performer Owned tune";

struct Search {
    directory: tempfile::TempDir,
    service: Service,
}

impl Search {
    fn new(limit: u16, songs: Value, videos: Value) -> Self {
        let directory = tempfile::tempdir().unwrap();
        let executable = directory.path().join("owned-yt-dlp");
        fs::write(&executable, include_str!("../fixtures/combined-search.sh")).unwrap();
        fs::set_permissions(&executable, fs::Permissions::from_mode(0o700)).unwrap();
        let mut songs_url = Url::parse("https://music.youtube.com/search").unwrap();
        songs_url.query_pairs_mut().append_pair("q", QUERY);
        songs_url.set_fragment(Some("songs"));
        let document = |entries: Value, url: String| {
            json!({"_type": "playlist", "original_url": url, "entries": entries}).to_string()
        };
        fs::write(
            directory.path().join("songs.json"),
            document(songs, songs_url.into()),
        )
        .unwrap();
        fs::write(
            directory.path().join("videos.json"),
            document(videos, format!("ytsearch{limit}:{QUERY}")),
        )
        .unwrap();
        Self {
            directory,
            service: Service::new(Config { executable }),
        }
    }

    fn path(&self, name: &str) -> PathBuf {
        self.directory.path().join(name)
    }

    fn mode(&self, origin: &str, mode: &str) {
        fs::write(self.path(&format!("{origin}.mode")), mode).unwrap();
    }

    async fn search(&self, limit: u16) -> (StatusCode, Vec<u8>) {
        let response = self
            .service
            .router()
            .oneshot(
                Request::post("/v1/search")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        json!({"query": QUERY, "cursor": null, "limit": limit}).to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        let status = response.status();
        let bytes = response.into_body().collect().await.unwrap().to_bytes();
        let body = std::str::from_utf8(&bytes).unwrap();
        assert!(
            !body.contains("private-") && !body.contains(self.directory.path().to_str().unwrap()),
            "search responses must not expose extractor diagnostics or owned paths"
        );
        (status, bytes.to_vec())
    }

    async fn page(&self, limit: u16) -> Vec<(String, String, Option<u64>)> {
        let (status, bytes) = self.search(limit).await;
        assert_eq!(status, StatusCode::OK);
        let page: SearchPage = serde_json::from_slice(&bytes).unwrap();
        assert!(page.validate(limit).is_ok() && page.next_cursor.is_none());
        page.items
            .into_iter()
            .map(|item| (item.id, item.artist, item.duration_ms))
            .collect()
    }

    fn invocations(&self) -> usize {
        fs::read_to_string(self.path("calls"))
            .unwrap()
            .lines()
            .count()
    }

    async fn guarded(&self, body: impl Future<Output = ()>) {
        let result = tokio::time::timeout(
            Duration::from_secs(15),
            AssertUnwindSafe(body).catch_unwind(),
        )
        .await;
        let _ = fs::write(self.path("release"), "");
        let cleanup = tokio::time::timeout(Duration::from_secs(3), self.service.shutdown()).await;
        assert!(
            matches!(cleanup, Ok(Ok(()))),
            "the search service must shut down and join its extraction workers"
        );
        match result {
            Ok(Ok(())) => {}
            Ok(Err(panic)) => std::panic::resume_unwind(panic),
            Err(_) => panic!("the combined search exercise must settle within its owned deadline"),
        }
    }
}

fn song(id: &str, title: &str) -> Value {
    json!({"id": id, "title": title})
}

fn video(id: &str, title: &str, channel: &str, duration: u32) -> Value {
    json!({"id": id, "title": title, "channel": channel, "uploader": channel, "duration": duration})
}

fn catalog() -> Value {
    json!([
        song("OwnedSong00", "Owned other tune"),
        song("OwnedSong01", "Owned tune"),
        song("OwnedSong02", "Owned third tune")
    ])
}

fn unknown(id: &str) -> (String, String, Option<u64>) {
    (id.to_owned(), "Unknown artist".to_owned(), None)
}

fn channel(id: &str, name: &str, milliseconds: u64) -> (String, String, Option<u64>) {
    (
        id.to_owned(),
        format!("Channel: {name}"),
        Some(milliseconds),
    )
}

#[tokio::test]
async fn one_video_naming_every_query_word_leads_and_catalog_songs_keep_their_order() {
    let search = Search::new(
        10,
        catalog(),
        json!([
            video("OwnedClip00", "Owned unrelated clip", "Owned clips", 90),
            video(
                "OwnedTune01",
                "Owned performer - Owned tune",
                "Owned label",
                365
            ),
            video(
                "OwnedLyric1",
                "Owned performer - Owned tune (Lyrics)",
                "Owned lyrics",
                331
            )
        ]),
    );
    search.guarded(async {
    let ranked = search.page(10).await;
    assert_eq!(
        ranked,
        [
            channel("OwnedTune01", "Owned label", 365_000),
            unknown("OwnedSong00"),
            unknown("OwnedSong01"),
            unknown("OwnedSong02"),
            channel("OwnedClip00", "Owned clips", 90_000),
            channel("OwnedLyric1", "Owned lyrics", 331_000),
        ],
        "only the first video naming every query word may precede the catalog songs, and the other videos follow them"
    );
    let arguments = fs::read(search.path("last-argv")).unwrap();
    let arguments = arguments
        .split(|byte| *byte == 0)
        .map(|argument| std::str::from_utf8(argument).unwrap())
        .collect::<Vec<_>>();
    let separator = arguments.iter().position(|argument| *argument == "--").unwrap();
    assert!(
        arguments[separator + 1].starts_with("https://music.youtube.com/search?q=")
            && arguments[separator + 2] == format!("ytsearch10:{QUERY}")
            && arguments[..separator].contains(&"--ignore-errors"),
        "the catalog search must run before the added video search, and a failed target must not stop the other"
    );
    assert_eq!(search.invocations(), 1);
    })
    .await;
}

#[tokio::test]
async fn without_a_video_naming_every_query_word_catalog_songs_come_first() {
    let search = Search::new(
        10,
        catalog(),
        json!([
            video("OwnedLyric1", "Owned tune (Lyrics)", "Owned lyrics", 331),
            video(
                "OwnedClip00",
                "Owned performer interview",
                "Owned clips",
                90
            )
        ]),
    );
    search.guarded(async {
    assert_eq!(
        search.page(10).await,
        [
            unknown("OwnedSong00"),
            unknown("OwnedSong01"),
            unknown("OwnedSong02"),
            channel("OwnedLyric1", "Owned lyrics", 331_000),
            channel("OwnedClip00", "Owned clips", 90_000),
        ],
        "a video missing any query word must not displace a catalog song from the leading window"
    );
    })
    .await;
}

#[tokio::test]
async fn the_limit_bounds_the_combined_ranking() {
    let search = Search::new(
        2,
        catalog(),
        json!([video(
            "OwnedTune01",
            "Owned performer - Owned tune",
            "Owned label",
            365
        )]),
    );
    search
        .guarded(async {
            assert_eq!(
                search.page(2).await,
                [
                    channel("OwnedTune01", "Owned label", 365_000),
                    unknown("OwnedSong00")
                ]
            );
        })
        .await;
}

#[tokio::test]
async fn an_identity_in_both_results_keeps_catalog_evidence_and_only_fills_missing_fields() {
    let search = Search::new(
        10,
        json!([
            {"id": "OwnedRich01", "title": "Owned tune", "artists": ["Owned performer", "Owned guest"], "duration": 333},
            {"id": "OwnedBare02", "title": "Owned b-side"},
            {"id": "OwnedConf03", "title": "Owned closer", "artist": "Owned performer"}
        ]),
        json!([
            {"id": "OwnedRich01", "title": "Owned performer - Owned tune", "channel": "Owned label"},
            video("OwnedBare02", "Owned performer - Owned b-side", "Owned performer - Topic", 200),
            {"id": "OwnedConf03", "title": "Owned closer", "artist": "Owned impostor", "channel": "Owned label", "duration": 100}
        ]),
    );
    search.guarded(async {
    let (status, bytes) = search.search(10).await;
    assert_eq!(status, StatusCode::OK);
    let page: SearchPage = serde_json::from_slice(&bytes).unwrap();
    let items = page
        .items
        .iter()
        .map(|item| {
            (
                item.id.as_str(),
                item.title.as_str(),
                item.artist.as_str(),
                item.artists.len(),
                item.duration_ms,
            )
        })
        .collect::<Vec<_>>();
    assert_eq!(
        items,
        [
            ("OwnedRich01", "Owned tune", "Owned performer, Owned guest", 2, Some(333_000)),
            ("OwnedBare02", "Owned b-side", "Channel: Owned performer - Topic", 0, Some(200_000)),
            ("OwnedConf03", "Owned closer", "Owned performer", 0, Some(100_000)),
        ],
        "a catalog row's title, artists and duration must win, a video may only fill absent fields, and a channel stays marked as a channel"
    );
    })
    .await;
}

#[tokio::test]
async fn a_failed_video_search_keeps_the_catalog_songs() {
    let search = Search::new(10, catalog(), json!([]));
    search.mode("videos", "fail");
    search
        .guarded(async {
            assert_eq!(
                search.page(10).await,
                [
                    unknown("OwnedSong00"),
                    unknown("OwnedSong01"),
                    unknown("OwnedSong02")
                ]
            );
        })
        .await;
}

#[tokio::test]
async fn a_failed_catalog_search_keeps_the_videos() {
    let search = Search::new(
        10,
        catalog(),
        json!([video(
            "OwnedTune01",
            "Owned performer - Owned tune",
            "Owned label",
            365
        )]),
    );
    search.mode("songs", "fail");
    search
        .guarded(async {
            assert_eq!(
                search.page(10).await,
                [channel("OwnedTune01", "Owned label", 365_000)]
            );
        })
        .await;
}

#[tokio::test]
async fn both_searches_failing_is_not_an_empty_success() {
    let search = Search::new(10, catalog(), json!([]));
    search.mode("songs", "fail");
    search.mode("videos", "fail");
    search
        .guarded(async {
            let (status, _) = search.search(10).await;
            assert_ne!(status, StatusCode::OK);
        })
        .await;
}

#[tokio::test]
async fn a_malformed_second_document_keeps_the_first() {
    let search = Search::new(10, catalog(), json!([]));
    search.mode("videos", "malformed");
    search
        .guarded(async {
            assert_eq!(
                search.page(10).await,
                [
                    unknown("OwnedSong00"),
                    unknown("OwnedSong01"),
                    unknown("OwnedSong02")
                ]
            );
        })
        .await;
}

#[tokio::test]
async fn a_malformed_first_document_keeps_the_second() {
    let search = Search::new(
        10,
        catalog(),
        json!([video(
            "OwnedTune01",
            "Owned performer - Owned tune",
            "Owned label",
            365
        )]),
    );
    search.mode("songs", "malformed");
    search
        .guarded(async {
            assert_eq!(
                search.page(10).await,
                [channel("OwnedTune01", "Owned label", 365_000)],
                "a malformed catalog document must not swallow the complete video document after it"
            );
        })
        .await;
}

#[tokio::test]
async fn a_document_with_an_invalid_entry_is_dropped_without_discarding_the_other() {
    let search = Search::new(
        10,
        json!([song("bad/video01", "Owned invalid video")]),
        json!([video(
            "OwnedTune01",
            "Owned performer - Owned tune",
            "Owned label",
            365
        )]),
    );
    search
        .guarded(async {
            assert_eq!(
                search.page(10).await,
                [channel("OwnedTune01", "Owned label", 365_000)]
            );
        })
        .await;
}

#[tokio::test]
async fn a_hanging_video_search_returns_the_catalog_songs_at_the_deadline_and_stops_its_group() {
    let search = Search::new(10, catalog(), json!([]));
    search.mode("videos", "hang");
    search
        .guarded(async {
            let started = Instant::now();
            let ranked = tokio::time::timeout(Duration::from_secs(12), search.page(10))
                .await
                .expect("a hanging target must settle at the extraction deadline");
            assert!(started.elapsed() >= Duration::from_secs(7));
            assert_eq!(
                ranked,
                [
                    unknown("OwnedSong00"),
                    unknown("OwnedSong01"),
                    unknown("OwnedSong02")
                ]
            );
            let pid = fs::read_to_string(search.path("hanging.pid")).unwrap();
            assert!(
                !Path::new(&format!("/proc/{}", pid.trim())).exists(),
                "the hanging extraction must be stopped and reaped before the response"
            );
        })
        .await;
}

#[tokio::test]
async fn a_hanging_catalog_search_still_times_out() {
    let search = Search::new(
        10,
        catalog(),
        json!([video(
            "OwnedTune01",
            "Owned performer - Owned tune",
            "Owned label",
            365
        )]),
    );
    search.mode("songs", "hang");
    search
        .guarded(async {
            let (status, _) = tokio::time::timeout(Duration::from_secs(12), search.search(10))
                .await
                .expect("a hanging catalog target must settle at the extraction deadline");
            assert_eq!(status, StatusCode::GATEWAY_TIMEOUT);
        })
        .await;
}
