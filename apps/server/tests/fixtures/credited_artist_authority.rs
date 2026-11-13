#[path = "artwork_storage.rs"]
mod storage;

use axum::http::{HeaderMap, StatusCode};
use hito_plugin_sdk::{ArtworkEntity, ArtworkRequest, PlaylistItemsRequest};
use hito_testkit::fake_plugin::{Gate, Reply};
use serde_json::json;
use std::{sync::Arc, time::Duration};
pub use storage::{COVER, Library, Response, Storage};
use tokio::{sync::oneshot, task::JoinHandle};

pub const TOKEN: &str = "owned-credited-photo-credential-secret";
pub const PROVIDER_ARTIST: &str = "owned:storage/artist";

struct Pending {
    gate: Arc<Gate>,
    stop: Option<oneshot::Sender<()>>,
    task: JoinHandle<hito_core::Result<bool>>,
    entered: bool,
}

pub struct Owned {
    pub library: Library,
    pending: Option<Pending>,
}

impl Owned {
    pub fn new() -> Self {
        Self {
            library: Library::retain(),
            pending: None,
        }
    }

    pub async fn open(&mut self) {
        self.library.open().await;
        self.library.provider().on("v1/playlists/items", |request| {
            let parsed: PlaylistItemsRequest = serde_json::from_value(request.json()).unwrap();
            parsed.validate().unwrap();
            assert_eq!(parsed.playlist_id, "owned:storage/playlist");
            assert_eq!(parsed.snapshot, "owned-storage:1");
            assert!(parsed.cursor.is_none());
            assert_eq!(parsed.limit, 100);
            let item = json!({"kind": "recording", "recording": {
                "id": "owned:storage/recording", "title": "Owned storage recording",
                "artist": "Owned storage performer", "duration_ms": 120_000,
                "revision": "owned-storage-recording:1",
                "release": {"id": "owned:storage/release", "title": "Owned storage album"},
                "artists": [{"name": "Owned storage performer", "role": "main",
                    "provider_artist_id": PROVIDER_ARTIST}]
            }});
            Reply::Json(
                json!({"playlist_id": "owned:storage/playlist", "snapshot": "owned-storage:1",
                "next_cursor": null, "items": [
                    {"occurrence_id": "first", "item": item},
                    {"occurrence_id": "again", "item": item}]}),
            )
        });
        self.library.provider().on("v1/artwork", |request| {
            let parsed: ArtworkRequest = serde_json::from_value(request.json()).unwrap();
            parsed.validate().unwrap();
            assert!(matches!(parsed.entity, ArtworkEntity::Artist));
            assert_eq!(parsed.provider_id, PROVIDER_ARTIST);
            assert_eq!(request.protocol().as_deref(), Some("1"));
            assert!(request.query.is_none());
            assert!(
                request
                    .headers
                    .get("authorization")
                    .is_some_and(|value| value == format!("Bearer {TOKEN}").as_str())
            );
            let mut headers = HeaderMap::new();
            headers.insert("content-type", "image/png".parse().unwrap());
            Reply::Raw {
                status: StatusCode::OK,
                headers,
                body: COVER.to_vec(),
                chunked: false,
            }
        });
    }

    pub fn start(&mut self) {
        assert!(self.pending.is_none());
        let gate = self.library.provider().hold_next("v1/artwork");
        let catalog = self.library.hito().catalog.clone();
        let (stop, stopped) = oneshot::channel();
        let task = tokio::spawn(async move {
            catalog
                .run_next_cover_art_lookup_until(async {
                    let _ = stopped.await;
                })
                .await
        });
        self.pending = Some(Pending {
            gate,
            stop: Some(stop),
            task,
            entered: false,
        });
    }

    pub async fn entered(&mut self) -> bool {
        let pending = self.pending.as_mut().unwrap();
        pending.entered = pending.gate.entered_within(Duration::from_secs(5)).await;
        pending.entered && !pending.task.is_finished()
    }

    pub async fn complete(&mut self) -> bool {
        let pending = self.pending.as_ref().unwrap();
        assert!(
            !pending.gate.has_retired(),
            "The credential-race setup lost its held response before explicit release"
        );
        assert!(
            !pending.task.is_finished(),
            "The credential-race setup lost its running worker before explicit release"
        );
        pending.gate.open();
        let completed = tokio::time::timeout(
            Duration::from_secs(3),
            &mut self.pending.as_mut().unwrap().task,
        )
        .await;
        match completed {
            Ok(result) => {
                let pending = self.pending.take().unwrap();
                matches!(result, Ok(Ok(true))) && (!pending.entered || pending.gate.has_retired())
            }
            Err(_) => false,
        }
    }

    pub async fn close(mut self) {
        let retired = if let Some(mut pending) = self.pending.take() {
            pending.gate.open();
            if let Some(stop) = pending.stop.take() {
                let _ = stop.send(());
            }
            let timely = match tokio::time::timeout(Duration::from_secs(3), &mut pending.task).await
            {
                Ok(_) => true,
                Err(_) => {
                    let _ = pending.task.await;
                    false
                }
            };
            let held_retired =
                !pending.entered || pending.gate.retired_within(Duration::from_secs(3)).await;
            timely && held_retired
        } else {
            true
        };
        self.library.close().await;
        assert!(
            retired,
            "The held artist request and Core worker must join within their existing retirement bounds"
        );
    }
}
