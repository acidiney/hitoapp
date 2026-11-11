use crate::{fixture, lrclib, provider};
use axum::http::StatusCode;
use hito_core::{Action, Operation};
use serde_json::{Value, json};
use uuid::Uuid;

pub async fn commit_metadata(library: &fixture::Library, title: &str, artist: &str) {
    commit_metadata_for(library, &library.account, title, artist).await;
}

pub async fn commit_metadata_for(
    library: &fixture::Library,
    account: &crate::account::Account,
    title: &str,
    artist: &str,
) {
    let recording_id = Uuid::parse_str(&library.recording).unwrap();
    let operations = vec![
        Operation {
            id: Uuid::new_v4(),
            action: Action::RecordingTitleSet {
                recording_id,
                title: title.into(),
            },
        },
        Operation {
            id: Uuid::new_v4(),
            action: Action::RecordingArtistSet {
                recording_id,
                artist: artist.into(),
            },
        },
    ];
    let (status, _) = account
        .request(
            &library.app,
            "POST",
            "/api/v1/sync",
            json!({
                "device_id":"owned-automatic-lyrics-client", "operations":operations,
            }),
        )
        .await;
    assert_eq!(status, StatusCode::OK);
}

pub struct Prepared {
    pub library: fixture::Library,
    pub provider: provider::Provider,
    pub installation: Value,
}
impl Prepared {
    pub async fn new() -> Self {
        Self::with_duration(1).await
    }
    pub async fn with_duration(seconds: u32) -> Self {
        let provider = provider::Provider::start().await;
        let library = fixture::Library::prepare_with_duration(seconds).await;
        commit_metadata(&library, lrclib::TITLE, lrclib::ARTIST).await;
        let installation = library
            .account
            .connect(
                &library.app,
                provider.url(),
                "Owned automatic service",
                None,
            )
            .await;
        Self {
            library,
            provider,
            installation,
        }
    }
    pub fn path(&self) -> String {
        format!(
            "/api/v1/recordings/{}/lyrics/automatic",
            self.library.recording
        )
    }
    pub async fn read(&self) -> Value {
        let (status, value) = self
            .library
            .account
            .request(&self.library.app, "POST", &self.path(), json!({}))
            .await;
        assert_eq!(status, StatusCode::OK);
        value
    }
    pub async fn actor(&self) -> hito_core::Actor {
        let headers = self.library.account.headers();
        let cookie = headers["cookie"].to_str().unwrap();
        self.library
            .services
            .identity
            .authenticate(cookie.strip_prefix("hito_session=").unwrap())
            .await
            .unwrap()
    }
    pub async fn finish(self) {
        self.library.assert_original();
        self.provider.finish().await;
    }
}
