use crate::artwork_thousand_corpus::{COUNT, content_type};
use axum::http::{HeaderMap, StatusCode};
use hito_plugin_sdk::{
    ArtworkEntity, ArtworkRequest, Capability, Manifest, PlaylistDetailRequest,
    PlaylistItemsRequest,
};
use hito_testkit::fake_plugin::{FakePlugin, Reply};
use serde_json::json;
use std::path::PathBuf;

pub async fn start(directory: PathBuf) -> FakePlugin {
    FakePlugin::builder(Manifest {
        id: "org.hito.owned-artwork-thousand".into(),
        name: "Owned artwork capacity".into(),
        protocol_version: 1,
        capabilities: [Capability::Playlists, Capability::Artwork].into(),
        authorization: None,
        content_policy: None,
    })
    .require_protocol()
    .require_json()
    .post("v1/playlists/detail", |request| {
        let parsed: PlaylistDetailRequest = serde_json::from_value(request.json()).unwrap();
        parsed.validate().unwrap();
        let name = match parsed {
            PlaylistDetailRequest::Reference { reference } => {
                if reference == "https://provider.invalid/capacity/bulk" { "bulk" }
                else { assert!(reference == "https://provider.invalid/capacity/sentinel"); "sentinel" }
            },
            PlaylistDetailRequest::Canonical { playlist_id } => { assert!(playlist_id == "bulk" || playlist_id == "sentinel"); return Reply::Json(detail(&playlist_id)); }
        };
        Reply::Json(detail(name))
    })
    .post("v1/playlists/items", |request| {
        let parsed: PlaylistItemsRequest = serde_json::from_value(request.json()).unwrap();
        parsed.validate().unwrap();
        assert!(parsed.snapshot == "owned-capacity:1");
        let count = if parsed.playlist_id == "bulk" { COUNT } else { assert!(parsed.playlist_id == "sentinel"); 1 };
        let offset: usize = parsed.cursor.as_deref().map(|s| s.parse().unwrap()).unwrap_or(0);
        assert!(offset < count);
        let end = (offset + usize::from(parsed.limit)).min(count);
        let items = (offset..end).map(|position| {
            let index = if count == 1 { COUNT } else { position };
            json!({"occurrence_id": format!("owned-occurrence-{index}"), "item": {"kind": "recording", "recording": {
                "id": format!("owned:capacity/recording/{index}"), "title": format!("Owned capacity recording {index:04}"),
                "artist": "Owned capacity performer", "duration_ms": 120_000, "revision": "owned-capacity:1",
                "release": {"id": format!("owned:capacity/release/{index}"), "title": format!("Owned capacity album {index:04}")}
            }}})
        }).collect::<Vec<_>>();
        Reply::Json(json!({"playlist_id": parsed.playlist_id, "snapshot": parsed.snapshot, "items": items,
            "next_cursor": (end < count).then(|| end.to_string())}))
    })
    .post("v1/artwork", move |request| {
        let parsed: ArtworkRequest = serde_json::from_value(request.json()).unwrap();
        parsed.validate().unwrap();
        let prefix = match parsed.entity {
            ArtworkEntity::Release => "owned:capacity/release/",
            ArtworkEntity::Recording => "owned:capacity/recording/",
            _ => return Reply::Refuse,
        };
        let index: usize = parsed.provider_id.strip_prefix(prefix).unwrap().parse().unwrap();
        assert!(index <= COUNT);
        let (bytes, mime) = if index == COUNT {
            if matches!(parsed.entity, ArtworkEntity::Recording) {
                (include_bytes!("cover.png").to_vec(), "image/png")
            } else {
                (include_bytes!("artwork-normalization-1024x768.png").to_vec(), "image/png")
            }
        } else {
            assert!(matches!(parsed.entity, ArtworkEntity::Release));
            (std::fs::read(directory.join(index.to_string())).unwrap(), content_type(index))
        };
        let mut headers = HeaderMap::new();
        headers.insert("content-type", mime.parse().unwrap());
        Reply::Raw { status: StatusCode::OK, headers, body: bytes, chunked: false }
    })
    .strict(1100)
    .start().await
}

fn detail(name: &str) -> serde_json::Value {
    json!({"id": name, "name": "Owned capacity selection", "snapshot": "owned-capacity:1",
        "total_count": if name == "bulk" { COUNT } else { 1 }, "attribution": {"label": "Owned capacity fixture"}})
}
