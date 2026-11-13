use axum::{
    Router,
    body::{Body, to_bytes},
    http::{HeaderMap, Request, StatusCode},
};
use hito_core::{
    CoreConfig, HostDeviceId, Services,
    test_support::{database_owner::DatabaseOwner, support},
};
use hito_plugin_sdk::{
    ArtworkEntity, ArtworkRequest, Capability, Manifest, PlaylistDetailRequest,
    PlaylistItemsRequest,
};
use hito_testkit::fake_plugin::{FakePlugin, Reply};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use sqlx::{Connection, SqliteConnection, sqlite::SqliteConnectOptions};
use std::{sync::Arc, time::Duration};
use tower::ServiceExt;

pub const ARTIST: &str = "0f820000-0000-4000-8000-0000000000a1";
pub const COVER: &[u8] = hito_core::test_support::covers::COVER_A;
const PLAYLIST: &str = "owned:storage/playlist";
const SNAPSHOT: &str = "owned-storage:1";

pub struct Response {
    pub status: StatusCode,
    pub headers: HeaderMap,
    pub bytes: Vec<u8>,
}

impl Response {
    pub fn json(&self) -> Value {
        serde_json::from_slice(&self.bytes).unwrap()
    }
}

#[derive(Debug)]
pub struct Storage {
    pub assets: i64,
    pub bytes: i64,
    pub pending: i64,
    pub reserved: i64,
    pub current: i64,
    pub account_counts: Vec<(i64, i64)>,
    pub broken_references: i64,
    pub foreign_key_failures: usize,
}

pub struct Library {
    directory: Arc<tempfile::TempDir>,
    database: DatabaseOwner,
    services: Option<Services>,
    app: Option<Router>,
    provider: Option<FakePlugin>,
    original_audio: Vec<u8>,
    inspector: Option<SqliteConnection>,
}

impl Library {
    pub fn retain() -> Self {
        let directory = Arc::new(tempfile::tempdir().unwrap());
        let database = DatabaseOwner::retain(directory.clone());
        Self {
            directory,
            database,
            services: None,
            app: None,
            provider: None,
            original_audio: Vec::new(),
            inspector: None,
        }
    }

    pub async fn open(&mut self) {
        let image = image::load_from_memory(COVER).unwrap();
        assert!(image.width() > 0 && image.height() > 0);
        assert!(image.width().max(image.height()) <= 600);
        assert!(!COVER.is_empty() && COVER.len() <= 2 * 1024 * 1024);
        let music = self.directory.path().join("music");
        std::fs::create_dir(&music).unwrap();
        self.original_audio = support::write_wav(&music.join("Owned.wav"), 0);
        std::fs::write(music.join("cover.png"), COVER).unwrap();
        for name in ["Owned.wav", "cover.png"] {
            let path = music.join(name);
            let mut permissions = std::fs::metadata(&path).unwrap().permissions();
            permissions.set_readonly(true);
            std::fs::set_permissions(path, permissions).unwrap();
        }
        self.provider = Some(renderer().await);
        let mut config = CoreConfig::new(
            self.directory.path().join("hito.db"),
            music,
            HostDeviceId::new("artwork-global-reservation").unwrap(),
        );
        config.musicbrainz_loopback_endpoint = Some(self.provider().url.parse().unwrap());
        config.artist_image_loopback_endpoint = Some(self.provider().url.parse().unwrap());
        self.database.attach(&mut config);
        self.services = Some(hito_core::open(config).await.unwrap());
        self.app = Some(hito_server::router(self.hito().clone()));
        let options = SqliteConnectOptions::new()
            .filename(self.directory.path().join("hito.db"))
            .read_only(true)
            .create_if_missing(false);
        self.inspector = Some(SqliteConnection::connect_with(&options).await.unwrap());
    }

    pub fn hito(&self) -> &Services {
        self.services.as_ref().unwrap()
    }

    pub fn provider(&self) -> &FakePlugin {
        self.provider.as_ref().unwrap()
    }

    pub async fn exchange(
        &self,
        token: Option<&str>,
        method: &str,
        path: &str,
        body: Value,
    ) -> Response {
        tokio::time::timeout(Duration::from_secs(5), async {
            let mut request = Request::builder()
                .method(method)
                .uri(path)
                .header("content-type", "application/json")
                .header("x-hito-request", "1");
            if let Some(token) = token {
                request = request.header("authorization", format!("Bearer {token}"));
            }
            let response = self
                .app
                .as_ref()
                .unwrap()
                .clone()
                .oneshot(request.body(Body::from(body.to_string())).unwrap())
                .await
                .unwrap();
            let status = response.status();
            let headers = response.headers().clone();
            let bytes = to_bytes(response.into_body(), 2 * 1024 * 1024)
                .await
                .unwrap()
                .to_vec();
            Response {
                status,
                headers,
                bytes,
            }
        })
        .await
        .expect("The owned storage HTTP request must finish")
    }

    pub async fn get(&self, token: &str, path: &str) -> Value {
        let response = self.exchange(Some(token), "GET", path, Value::Null).await;
        assert_eq!(response.status, StatusCode::OK);
        response.json()
    }

    pub async fn storage(&mut self) -> Storage {
        let connection = self.inspector.as_mut().unwrap();
        let result: Result<Storage, sqlx::Error> = async {
            let (assets, bytes): (i64, i64) = sqlx::query_as(
                "SELECT count(*),coalesce(sum(length(bytes)),0) FROM artwork_assets",
            ).fetch_one(&mut *connection).await?;
            let (pending, reserved): (i64, i64) = sqlx::query_as(
                "SELECT count(*),coalesce(sum(reservation_bytes),0) FROM artwork_fetches WHERE status IN ('queued','running')",
            ).fetch_one(&mut *connection).await?;
            let current: i64 = sqlx::query_scalar(
                "SELECT count(*) FROM current_artwork_fetch_authority",
            ).fetch_one(&mut *connection).await?;
            let account_counts: Vec<(i64, i64)> = sqlx::query_as(
                "SELECT count(*),sum(reservation_bytes) FROM artwork_fetches WHERE status IN ('queued','running') GROUP BY user_id ORDER BY count(*),sum(reservation_bytes)",
            ).fetch_all(&mut *connection).await?;
            let broken_references: i64 = sqlx::query_scalar(
                "SELECT count(*) FROM (
                    SELECT r.user_id,r.content_hash FROM recording_artwork r
                    UNION ALL SELECT r.user_id,r.content_hash FROM release_artwork r
                    UNION ALL SELECT r.user_id,r.content_hash FROM playlist_artwork r
                    UNION ALL SELECT r.user_id,r.content_hash FROM cover_art_lookups r WHERE r.content_hash IS NOT NULL
                 ) r LEFT JOIN artwork_assets a ON a.user_id = r.user_id AND a.content_hash = r.content_hash WHERE a.content_hash IS NULL",
            ).fetch_one(&mut *connection).await?;
            let foreign_key_failures = sqlx::query("PRAGMA foreign_key_check")
                .fetch_all(&mut *connection).await?.len();
            Ok(Storage { assets, bytes, pending, reserved, current, account_counts,
                broken_references, foreign_key_failures })
        }.await;
        result.unwrap()
    }

    pub fn assert_originals(&self) {
        let music = self.directory.path().join("music");
        assert_eq!(
            Sha256::digest(std::fs::read(music.join("Owned.wav")).unwrap()),
            Sha256::digest(&self.original_audio)
        );
        assert_eq!(
            Sha256::digest(std::fs::read(music.join("cover.png")).unwrap()),
            Sha256::digest(COVER)
        );
    }

    pub async fn close(mut self) {
        let inspector_closed = match self.inspector.take() {
            Some(connection) => connection.close().await.is_ok(),
            None => true,
        };
        let provider_joined = match self.provider.as_mut() {
            Some(provider) => provider.finish().await,
            None => true,
        };
        drop(self.app.take());
        let closed = if let Some(hito) = self.services.take() {
            let playback = hito.playback.close().await;
            let replicas = hito.replicas.close().await;
            let replication = hito.catalog_replication.close().await;
            drop(hito);
            [playback.is_ok(), replicas.is_ok(), replication.is_ok()]
        } else {
            [true; 3]
        };
        let retired = self.database.retire(Duration::from_secs(3)).await;
        assert!(inspector_closed && provider_joined && closed.into_iter().all(|closed| closed));
        assert!(retired.resources_retired && !retired.deadline_exceeded);
        assert_eq!(self.database.observe().active_resources(), 0);
    }
}

async fn renderer() -> FakePlugin {
    FakePlugin::builder(Manifest {
        id: "org.hito.owned-artwork-storage".into(),
        name: "Owned artwork storage".into(),
        protocol_version: 1,
        capabilities: [Capability::Playlists, Capability::Artwork].into(),
        authorization: None,
        content_policy: None,
    })
    .post("v1/playlists/detail", |request| {
        assert_eq!(request.protocol().as_deref(), Some("1"));
        let parsed: PlaylistDetailRequest = serde_json::from_value(request.json()).unwrap();
        parsed.validate().unwrap();
        Reply::Json(json!({"id": PLAYLIST, "name": "Owned storage selection",
            "snapshot": SNAPSHOT, "total_count": 2,
            "attribution": {"label": "Owned artwork storage"}}))
    })
    .post("v1/playlists/items", |request| {
        assert_eq!(request.protocol().as_deref(), Some("1"));
        let parsed: PlaylistItemsRequest = serde_json::from_value(request.json()).unwrap();
        parsed.validate().unwrap();
        assert_eq!(parsed.playlist_id, PLAYLIST);
        assert_eq!(parsed.snapshot, SNAPSHOT);
        assert!(parsed.cursor.is_none());
        let item = json!({"kind": "recording", "recording": {
            "id": "owned:storage/recording", "title": "Owned storage recording",
            "artist": "Owned storage performer", "duration_ms": 120_000,
            "revision": "owned-storage-recording:1",
            "release": {"id": "owned:storage/release", "title": "Owned storage album"}
        }});
        Reply::Json(json!({"playlist_id": PLAYLIST, "snapshot": SNAPSHOT,
            "next_cursor": null, "items": [
                {"occurrence_id": "first", "item": item},
                {"occurrence_id": "again", "item": item}]}))
    })
    .post("v1/artwork", |request| {
        let parsed: ArtworkRequest = serde_json::from_value(request.json()).unwrap();
        parsed.validate().unwrap();
        assert!(matches!(parsed.entity, ArtworkEntity::Playlist));
        assert_eq!(parsed.provider_id, PLAYLIST);
        png()
    })
    .get(&format!("artist/{ARTIST}"), |request| {
        assert_eq!(request.query.as_deref(), Some("fmt=json&inc=url-rels"));
        assert!(!request.has_authorization());
        Reply::Json(json!({"id": ARTIST, "relations": [
            {"type": "wikidata", "url": {"resource": "https://www.wikidata.org/wiki/Q42"}}
        ]}))
    })
    .get("wiki/Special:EntityData/Q42.json", |request| {
        assert!(request.query.is_none() && !request.has_authorization());
        Reply::Json(json!({"entities": {"Q42": {"claims": {"P18": [
            {"rank": "normal", "mainsnak": {"datavalue": {"value": "Owned.png"}}}
        ]}}}}))
    })
    .get("wiki/Special:FilePath/Owned.png", |request| {
        assert_eq!(request.query.as_deref(), Some("width=320"));
        assert!(!request.has_authorization());
        png()
    })
    .require_json()
    .start()
    .await
}

fn png() -> Reply {
    let mut headers = HeaderMap::new();
    headers.insert("content-type", "image/png".parse().unwrap());
    Reply::Raw {
        status: StatusCode::OK,
        headers,
        body: COVER.to_vec(),
        chunked: false,
    }
}
