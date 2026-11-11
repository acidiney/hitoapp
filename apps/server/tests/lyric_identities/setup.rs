use crate::{fixture, lyric_provider};
use axum::http::StatusCode;
use serde_json::{Value, json};

pub struct Prepared {
    pub provider: lyric_provider::Provider,
    pub library: fixture::Library,
    pub installation: Value,
    pub identity: Value,
    pub candidate: Value,
    pub accept: Value,
}
impl Prepared {
    pub async fn new() -> Self {
        let provider = lyric_provider::Provider::start().await;
        let library = fixture::Library::prepare().await;
        let installation = library
            .account
            .connect(&library.app, &provider.url, "Independent lyrics", None)
            .await;
        let base = format!("/api/v1/recordings/{}/lyrics", library.recording);
        let candidate = json!({"installation_id":installation["id"],"installation_revision":installation["revision"],"provider_recording_id":"independent:recording"});
        let (status, review) = library
            .account
            .request(
                &library.app,
                "POST",
                &format!("{base}/identities/review"),
                candidate.clone(),
            )
            .await;
        assert_eq!(status, StatusCode::OK);
        let accept = json!({"candidate":candidate,"review_stamp":review["review_stamp"]});
        let (status, identity) = library
            .account
            .request(
                &library.app,
                "POST",
                &format!("{base}/identities"),
                accept.clone(),
            )
            .await;
        assert_eq!(status, StatusCode::OK);
        Self {
            provider,
            library,
            installation,
            identity,
            candidate,
            accept,
        }
    }
    pub fn base(&self) -> String {
        format!("/api/v1/recordings/{}/lyrics", self.library.recording)
    }
    pub fn read_body(&self) -> Value {
        json!({"selector":self.identity["selector"],"installation_revision":self.installation["revision"]})
    }
    pub async fn read(&self) -> Value {
        let (status, result) = self
            .library
            .account
            .request(
                &self.library.app,
                "POST",
                &format!("{}/identities/read", self.base()),
                self.read_body(),
            )
            .await;
        assert_eq!(status, StatusCode::OK);
        result
    }
}
