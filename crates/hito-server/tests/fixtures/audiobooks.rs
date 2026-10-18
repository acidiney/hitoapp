use crate::support::{call, indexed_app};
use axum::{
    Router,
    body::Body,
    http::{HeaderMap, Request, StatusCode},
};
use hito_core::{Action, Operation};
use http_body_util::BodyExt;
use serde_json::{Value, json};
use tower::ServiceExt;
use uuid::Uuid;

pub struct Auth {
    token: String,
    id: String,
}

pub struct Response {
    pub status: StatusCode,
    pub headers: HeaderMap,
    pub body: Value,
}

pub async fn access_page(
    app: &Router,
    path: &str,
    account: Option<&Auth>,
    shared_only: bool,
) -> Response {
    let mut request = Request::builder()
        .uri(path)
        .header("x-hito-request", "1")
        .header("origin", "http://localhost:8080")
        .header("x-inertia", "true");
    if let Some(account) = account {
        request = request
            .header("cookie", format!("hito_session={}", account.token))
            .header("x-hito-account", account.id.as_str());
    }
    if shared_only {
        request = request
            .header("x-inertia-partial-component", "AudiobookPage")
            .header("x-inertia-partial-data", "user,playlists");
    }
    let response = app
        .clone()
        .oneshot(request.body(Body::empty()).unwrap())
        .await
        .unwrap();
    let status = response.status();
    let headers = response.headers().clone();
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    Response {
        status,
        headers,
        body: serde_json::from_slice(&bytes).unwrap_or(Value::Null),
    }
}

impl Auth {
    pub async fn page(&self, app: &Router, path: &str) -> Response {
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri(path)
                    .header("cookie", format!("hito_session={}", self.token))
                    .header("x-hito-account", self.id.as_str())
                    .header("x-hito-request", "1")
                    .header("origin", "http://localhost:8080")
                    .header("x-inertia", "true")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        let status = response.status();
        let headers = response.headers().clone();
        let bytes = response.into_body().collect().await.unwrap().to_bytes();
        Response {
            status,
            headers,
            body: serde_json::from_slice(&bytes).unwrap_or(Value::Null),
        }
    }

    pub async fn get_claiming(&self, app: &Router, path: &str, claimed: &Auth) -> Response {
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri(path)
                    .header("cookie", format!("hito_session={}", self.token))
                    .header("x-hito-account", claimed.id.as_str())
                    .header("x-hito-request", "1")
                    .header("origin", "http://localhost:8080")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        let status = response.status();
        let headers = response.headers().clone();
        let bytes = response.into_body().collect().await.unwrap().to_bytes();
        Response {
            status,
            headers,
            body: serde_json::from_slice(&bytes).unwrap_or(Value::Null),
        }
    }

    pub async fn request(&self, app: &Router, method: &str, path: &str, body: Value) -> Response {
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method(method)
                    .uri(path)
                    .header("cookie", format!("hito_session={}", self.token))
                    .header("x-hito-account", self.id.as_str())
                    .header("x-hito-request", "1")
                    .header("origin", "http://localhost:8080")
                    .header("content-type", "application/json")
                    .body(if body.is_null() {
                        Body::empty()
                    } else {
                        Body::from(body.to_string())
                    })
                    .unwrap(),
            )
            .await
            .unwrap();
        let status = response.status();
        let headers = response.headers().clone();
        let bytes = response.into_body().collect().await.unwrap().to_bytes();
        Response {
            status,
            headers,
            body: serde_json::from_slice(&bytes).unwrap_or(Value::Null),
        }
    }

    pub async fn get(&self, app: &Router, path: &str) -> Value {
        let response = self.request(app, "GET", path, Value::Null).await;
        assert_eq!(response.status, StatusCode::OK);
        response.body
    }

    pub async fn sync(&self, app: &Router, actions: Vec<Action>) -> Vec<Value> {
        let operations: Vec<Operation> = actions
            .into_iter()
            .map(|action| Operation {
                id: Uuid::new_v4(),
                action,
            })
            .collect();
        let ids: Vec<Uuid> = operations.iter().map(|operation| operation.id).collect();
        let encoded = serde_json::to_value(&operations).unwrap();
        let response = self
            .request(
                app,
                "POST",
                "/api/v1/sync",
                json!({"device_id": "owned-audiobook-http-client", "operations": encoded}),
            )
            .await;
        assert_eq!(response.status, StatusCode::OK);
        assert!(
            response.body == json!({"acknowledged": ids}),
            "The HTTP sync adapter must retain every exact accepted operation acknowledgement"
        );
        encoded.as_array().unwrap().clone()
    }

    async fn state(&self, app: &Router, recording: &str, playlists: &[Uuid]) -> Value {
        let mut details = Vec::new();
        let mut downloads = Vec::new();
        for playlist in playlists {
            details.push(
                self.get(app, &format!("/api/v1/playlists/{playlist}"))
                    .await,
            );
            let response = self
                .request(
                    app,
                    "GET",
                    &format!("/api/v1/playlists/{playlist}/download"),
                    Value::Null,
                )
                .await;
            assert_eq!(response.status, StatusCode::NOT_FOUND);
            downloads.push(json!({"status": response.status.as_u16(), "body": response.body}));
        }
        let response = self
            .request(
                app,
                "GET",
                &format!("/api/v1/recordings/{recording}/download"),
                Value::Null,
            )
            .await;
        assert_eq!(response.status, StatusCode::NOT_FOUND);
        downloads.push(json!({"status": response.status.as_u16(), "body": response.body}));
        let progress = self
            .request(
                app,
                "POST",
                "/api/v1/recordings/progress",
                json!({"recording_ids": [recording]}),
            )
            .await;
        assert_eq!(progress.status, StatusCode::OK);
        json!({
            "libraries": self.get(app, "/api/v1/libraries").await,
            "recordings": self.get(app, "/api/v1/recordings").await,
            "artists": self.get(app, "/api/v1/artists").await,
            "releases": self.get(app, "/api/v1/releases").await,
            "metadata": self.get(app, &format!("/api/v1/recordings/{recording}/metadata")).await,
            "sources": self.get(app, &format!("/api/v1/recordings/{recording}/sources")).await,
            "playlists": self.get(app, "/api/v1/playlists").await,
            "details": details, "downloads": downloads,
            "activity": self.get(app, "/api/v1/activity").await,
            "progress": progress.body,
            "events": self.get(app, "/api/v1/sync?cursor=0").await,
        })
    }
}

pub struct Library {
    pub app: Router,
    pub owner: Auth,
    pub peer: Auth,
    pub recording: Uuid,
    pub book: Uuid,
    pub entries: [Uuid; 3],
    unrelated: Uuid,
    peer_book: Uuid,
    directory: tempfile::TempDir,
    original: Vec<u8>,
}

impl Library {
    pub async fn prepare() -> Self {
        let (directory, app, token, recording) = indexed_app().await;
        let original = std::fs::read(directory.path().join("music/Fixture.wav")).unwrap();
        let (status, user) = call(&app, "GET", "/api/v1/me", Value::Null, Some(&token)).await;
        assert_eq!(status, StatusCode::OK);
        let owner = Auth {
            token,
            id: user["id"].as_str().unwrap().into(),
        };
        let user = owner.request(&app, "POST", "/api/v1/users",
            json!({"username": "book-http-listener", "password": "owned audiobook HTTP listener password"})).await;
        assert_eq!(user.status, StatusCode::CREATED);
        let libraries = owner.get(&app, "/api/v1/libraries").await;
        let shared = libraries
            .as_array()
            .unwrap()
            .iter()
            .find(|library| library["personal"] == false)
            .unwrap();
        let grant = owner
            .request(
                &app,
                "PUT",
                &format!(
                    "/api/v1/libraries/{}/members/{}",
                    shared["id"].as_str().unwrap(),
                    user.body["id"].as_str().unwrap()
                ),
                Value::Null,
            )
            .await;
        assert_eq!(grant.status, StatusCode::NO_CONTENT);
        let (status, session) = call(&app, "POST", "/api/v1/login",
            json!({"username": "book-http-listener", "password": "owned audiobook HTTP listener password"}), None).await;
        assert_eq!(status, StatusCode::OK);
        let peer = Auth {
            token: session["token"].as_str().unwrap().into(),
            id: user.body["id"].as_str().unwrap().into(),
        };
        let recording = Uuid::parse_str(&recording).unwrap();
        let book = Uuid::new_v4();
        let unrelated = Uuid::new_v4();
        let peer_book = Uuid::new_v4();
        let entries = [Uuid::new_v4(), Uuid::new_v4(), Uuid::new_v4()];
        let mut actions = vec![
            Action::PlaylistCreate {
                playlist_id: book,
                name: "Private HTTP audiobook".into(),
            },
            Action::PlaylistCreate {
                playlist_id: unrelated,
                name: "Private ungrouped selection".into(),
            },
            Action::RecordingTitleSet {
                recording_id: recording,
                title: "Private HTTP chapter title".into(),
            },
            Action::RecordingArtistSet {
                recording_id: recording,
                artist: "Private HTTP narrator".into(),
            },
            Action::LikeSet {
                recording_id: recording,
                liked: true,
            },
            Action::ProgressSet {
                recording_id: recording,
                position_ms: 400,
                speed: 1.25,
            },
        ];
        for (entry_id, position) in entries.into_iter().zip([30, 10, 20]) {
            actions.push(Action::EntryInsert {
                playlist_id: book,
                entry_id,
                recording_id: recording,
                position,
            });
        }
        for position in 0..2 {
            actions.push(Action::EntryInsert {
                playlist_id: unrelated,
                entry_id: Uuid::new_v4(),
                recording_id: recording,
                position,
            });
        }
        owner.sync(&app, actions).await;
        peer.sync(
            &app,
            vec![
                Action::PlaylistCreate {
                    playlist_id: peer_book,
                    name: "Private other account book".into(),
                },
                Action::EntryInsert {
                    playlist_id: peer_book,
                    entry_id: Uuid::new_v4(),
                    recording_id: recording,
                    position: 0,
                },
                Action::AudiobookCreate {
                    playlist_id: peer_book,
                },
            ],
        )
        .await;
        Self {
            app,
            owner,
            peer,
            recording,
            book,
            entries,
            unrelated,
            peer_book,
            directory,
            original,
        }
    }

    pub async fn state(&self) -> Value {
        json!({
            "owner": self.owner.state(&self.app, &self.recording.to_string(), &[self.book, self.unrelated]).await,
            "peer": self.peer.state(&self.app, &self.recording.to_string(), &[self.peer_book]).await,
        })
    }

    pub async fn retained_state(&self) -> Value {
        let download = self
            .owner
            .request(
                &self.app,
                "GET",
                &format!("/api/v1/playlists/{}/download", self.book),
                Value::Null,
            )
            .await;
        assert_eq!(download.status, StatusCode::NOT_FOUND);
        json!({
            "owner": self.owner.state(&self.app, &self.recording.to_string(), &[self.unrelated]).await,
            "peer": self.peer.state(&self.app, &self.recording.to_string(), &[self.peer_book]).await,
            "book_download": {"status": download.status.as_u16(), "body": download.body},
        })
    }

    pub fn assert_original_unchanged(&self) {
        assert!(
            std::fs::read(self.directory.path().join("music/Fixture.wav")).unwrap()
                == self.original,
            "The audiobook HTTP workflow must retain every owned original WAV byte"
        );
    }
}
