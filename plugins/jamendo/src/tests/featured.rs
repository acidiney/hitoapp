use super::fixture::{ALBUM, ARTIST, CLIENT_ID, Provider, Server};
use crate::{Config, router};
use hito_plugin_sdk::{
    Capability, FeaturedKind, FeaturedPage, FeaturedRequest, PlaylistPage, ProviderPlaylist,
    ProviderPlaylistItem,
};
use serde_json::{Value, json};
use std::time::Duration;

struct Harness {
    provider: Provider,
    plugin: Server,
    client: reqwest::Client,
}

impl Harness {
    async fn start() -> Self {
        let provider = Provider::start().await;
        let config = Config::fixture(CLIENT_ID.to_owned(), provider.base.clone());
        let plugin = Server::start(router(config).expect("Jamendo router setup failed")).await;
        let client = reqwest::Client::builder()
            .no_proxy()
            .redirect(reqwest::redirect::Policy::none())
            .timeout(Duration::from_secs(3))
            .build()
            .expect("Jamendo fixture client setup failed");
        Self {
            provider,
            plugin,
            client,
        }
    }

    async fn post(&self, path: &str, body: Value) -> (reqwest::StatusCode, Vec<u8>) {
        let response = self
            .client
            .post(self.plugin.base.join(path).unwrap())
            .json(&body)
            .send()
            .await
            .expect("Jamendo plugin request failed");
        assert_eq!(response.headers()["cache-control"], "private, no-store");
        let status = response.status();
        (status, response.bytes().await.unwrap().to_vec())
    }
}

fn no_private_data(bytes: &[u8]) {
    let text = String::from_utf8_lossy(bytes).replace("https://www.jamendo.com/track/", "");
    for forbidden in [CLIENT_ID, "private-", "http://", "https://", "127.0.0.1"] {
        assert!(!text.contains(forbidden), "response exposed {forbidden}");
    }
}

#[tokio::test]
async fn the_manifest_declares_featured_music_that_opens_through_playlists() {
    let harness = Harness::start().await;
    let manifest: hito_plugin_sdk::Manifest = harness
        .client
        .get(harness.plugin.base.join("v1/manifest").unwrap())
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    manifest.validate().unwrap();
    assert_eq!(
        manifest.capabilities,
        [
            Capability::Search,
            Capability::Sources,
            Capability::Playlists,
            Capability::Featured
        ]
        .into()
    );
    assert_eq!(harness.provider.requests(), 0);
}

#[tokio::test]
async fn featured_albums_are_this_weeks_popular_albums_with_playlist_references() {
    let harness = Harness::start().await;
    let (status, bytes) = harness.post("v1/featured", json!({"limit": 12})).await;
    assert_eq!(status, reqwest::StatusCode::OK);
    no_private_data(&bytes);
    let page: FeaturedPage = serde_json::from_slice(&bytes).unwrap();
    page.validate_for(&FeaturedRequest { limit: 12 }).unwrap();
    assert_eq!(page.items.len(), 2);
    let first = &page.items[0];
    assert_eq!(first.id, "jamendo:album:v1:23");
    assert_eq!(first.reference, "jamendo:album:v1:23");
    assert_eq!(first.title, ALBUM);
    assert_eq!(first.subtitle.as_deref(), Some(ARTIST));
    assert_eq!(first.kind, FeaturedKind::Album);
    assert_eq!(first.artwork_id, None);
    assert_eq!(first.item_count, None);
    assert_eq!(harness.provider.album_requests(), 1);
    assert!(!harness.provider.rejected_request());
}

#[tokio::test]
async fn featured_requests_outside_the_contract_never_reach_jamendo() {
    let harness = Harness::start().await;
    for body in [
        json!({"limit": 0}),
        json!({"limit": 25}),
        json!({"limit": 5, "user": "x"}),
        json!({}),
    ] {
        let (status, _) = harness.post("v1/featured", body).await;
        assert_eq!(status, reqwest::StatusCode::BAD_REQUEST);
    }
    assert_eq!(harness.provider.requests(), 0);
}

#[tokio::test]
async fn a_featured_reference_imports_as_a_playlist_of_the_album_tracks() {
    let harness = Harness::start().await;
    let mut details = Vec::new();
    for body in [
        json!({"kind": "reference", "reference": "jamendo:album:v1:23"}),
        json!({"kind": "reference", "reference": "https://www.jamendo.com/album/23/owned-album"}),
        json!({"kind": "canonical", "playlist_id": "jamendo:album:v1:23"}),
    ] {
        let (status, bytes) = harness.post("v1/playlists/detail", body).await;
        assert_eq!(status, reqwest::StatusCode::OK);
        no_private_data(&bytes);
        let detail: ProviderPlaylist = serde_json::from_slice(&bytes).unwrap();
        detail.validate().unwrap();
        details.push(detail);
    }
    assert!(details.windows(2).all(|pair| pair[0] == pair[1]));
    let detail = &details[0];
    assert_eq!(detail.id, "jamendo:album:v1:23");
    assert_eq!(detail.name, ALBUM);
    assert_eq!(detail.total_count, 2);
    assert_eq!(detail.attribution.label, "Jamendo");

    let (status, bytes) = harness
        .post(
            "v1/playlists/items",
            json!({"playlist_id": detail.id, "snapshot": detail.snapshot, "cursor": null, "limit": 1}),
        )
        .await;
    assert_eq!(status, reqwest::StatusCode::OK);
    no_private_data(&bytes);
    let first: PlaylistPage = serde_json::from_slice(&bytes).unwrap();
    first.validate(1).unwrap();
    assert_eq!(first.next_cursor.as_deref(), Some("1"));
    let (status, bytes) = harness
        .post(
            "v1/playlists/items",
            json!({"playlist_id": detail.id, "snapshot": detail.snapshot, "cursor": "1", "limit": 100}),
        )
        .await;
    assert_eq!(status, reqwest::StatusCode::OK);
    let second: PlaylistPage = serde_json::from_slice(&bytes).unwrap();
    second.validate(100).unwrap();
    assert_eq!(second.next_cursor, None);
    let titles: Vec<String> = first
        .items
        .iter()
        .chain(&second.items)
        .map(|entry| match &entry.item {
            ProviderPlaylistItem::Recording { recording } => {
                assert_eq!(recording.artist, ARTIST);
                assert_eq!(recording.release.as_ref().unwrap().title, ALBUM);
                recording.id.clone()
            }
            _ => panic!("expected recordings"),
        })
        .collect();
    assert_eq!(titles, ["jamendo:track:v1:42", "jamendo:track:v1:43"]);
    assert!(!harness.provider.rejected_request());
}

#[tokio::test]
async fn invalid_references_and_stale_snapshots_are_rejected_safely() {
    let harness = Harness::start().await;
    for reference in [
        "https://example.com/album/23",
        "https://www.jamendo.com/artist/23",
        "jamendo:album:v1:abc",
        "jamendo:album:v1:0123",
        "jamendo:track:v1:42",
        "23",
    ] {
        let (status, _) = harness
            .post(
                "v1/playlists/detail",
                json!({"kind": "reference", "reference": reference}),
            )
            .await;
        assert_eq!(status, reqwest::StatusCode::BAD_REQUEST, "{reference}");
    }
    assert_eq!(harness.provider.requests(), 0);

    let (status, _) = harness
        .post(
            "v1/playlists/detail",
            json!({"kind": "reference", "reference": "jamendo:album:v1:99"}),
        )
        .await;
    assert_eq!(status, reqwest::StatusCode::NOT_FOUND);

    let (_, bytes) = harness
        .post(
            "v1/playlists/detail",
            json!({"kind": "canonical", "playlist_id": "jamendo:album:v1:23"}),
        )
        .await;
    let detail: ProviderPlaylist = serde_json::from_slice(&bytes).unwrap();
    harness.provider.change_album();
    let (status, _) = harness
        .post(
            "v1/playlists/items",
            json!({"playlist_id": detail.id, "snapshot": detail.snapshot, "cursor": null, "limit": 100}),
        )
        .await;
    assert_eq!(status, reqwest::StatusCode::CONFLICT);
    let (status, _) = harness
        .post(
            "v1/playlists/items",
            json!({"playlist_id": detail.id, "snapshot": detail.snapshot, "cursor": "x", "limit": 100}),
        )
        .await;
    assert_eq!(status, reqwest::StatusCode::BAD_REQUEST);
}
