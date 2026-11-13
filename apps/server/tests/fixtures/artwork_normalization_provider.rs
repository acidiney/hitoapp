use axum::http::{HeaderMap, StatusCode};
use hito_plugin_sdk::{
    ArtworkEntity, ArtworkRequest, Capability, Manifest, PlaylistDetailRequest,
    PlaylistItemsRequest,
};
use hito_testkit::fake_plugin::{FakePlugin, Reply};
use serde_json::json;

pub const COVER: &[u8] = include_bytes!("artwork-normalization-1024x768.png");
pub const RECORDING: &str = "owned:normalization/recording";
pub const RELEASE: &str = "owned:normalization/release";
const PLAYLIST: &str = "owned:normalization/playlist";
const SNAPSHOT: &str = "owned-normalization:1";

pub async fn start() -> FakePlugin {
    FakePlugin::builder(Manifest {
        id: "org.hito.owned-artwork-normalization".into(),
        name: "Owned artwork normalization".into(),
        protocol_version: 1,
        capabilities: [Capability::Playlists, Capability::Artwork].into(),
        authorization: None,
        content_policy: None,
    })
    .require_protocol()
    .post("v1/playlists/detail", |request| {
        let parsed: PlaylistDetailRequest = serde_json::from_value(request.json()).unwrap();
        parsed.validate().unwrap();
        Reply::Json(json!({
            "id": PLAYLIST, "name": "Owned artwork selection", "snapshot": SNAPSHOT,
            "total_count": 2, "attribution": {"label": "Owned artwork normalization"}
        }))
    })
    .post("v1/playlists/items", |request| {
        let parsed: PlaylistItemsRequest = serde_json::from_value(request.json()).unwrap();
        parsed.validate().unwrap();
        assert_eq!(parsed.playlist_id, PLAYLIST);
        assert_eq!(parsed.snapshot, SNAPSHOT);
        assert!(parsed.cursor.is_none());
        let item = json!({"kind": "recording", "recording": {
            "id": RECORDING, "title": "Owned recording", "artist": "Owned performer",
            "duration_ms": 120_000, "revision": "owned-recording:1",
            "release": {"id": RELEASE, "title": "Owned album"}
        }});
        Reply::Json(json!({
            "playlist_id": PLAYLIST, "snapshot": SNAPSHOT, "next_cursor": null,
            "items": [{"occurrence_id": "first", "item": item}, {"occurrence_id": "again", "item": item}]
        }))
    })
    .post("v1/artwork", |request| {
        let parsed: ArtworkRequest = serde_json::from_value(request.json()).unwrap();
        parsed.validate().unwrap();
        assert!(matches!((&parsed.entity, parsed.provider_id.as_str()),
            (ArtworkEntity::Recording, RECORDING) | (ArtworkEntity::Release, RELEASE)));
        let mut headers = HeaderMap::new();
        headers.insert("content-type", "image/png".parse().unwrap());
        Reply::Raw { status: StatusCode::OK, headers, body: COVER.to_vec(), chunked: false }
    })
    .strict(16)
    .start()
    .await
}
