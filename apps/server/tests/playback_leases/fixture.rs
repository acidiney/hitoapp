use axum::{
    Router,
    body::Body,
    http::{Request, header},
    response::Response,
};
use hito_core::{
    Actor, BeginPlaybackLease, CoreConfig, Credentials, HostDeviceId, Services,
    test_support::{database_owner::DatabaseOwner, support},
};
use std::{sync::Arc, time::Duration};
use tower::ServiceExt;

pub struct Case {
    pub directory: Arc<tempfile::TempDir>,
    owner: DatabaseOwner,
    pub services: Services,
    pub actor: Actor,
    pub app: Router,
    pub token: String,
    pub recording: String,
    pub source: String,
    pub original: Vec<u8>,
}

impl Case {
    pub async fn new() -> Self {
        Self::with_audio(|path| support::write_wav(path, 73)).await
    }

    pub async fn with_audio(write: impl FnOnce(&std::path::Path) -> Vec<u8>) -> Self {
        let directory = Arc::new(tempfile::tempdir().unwrap());
        let music = directory.path().join("music");
        std::fs::create_dir(&music).unwrap();
        let original = write(&music.join("private-owned.wav"));
        let owner = DatabaseOwner::retain(directory.clone());
        let mut config = CoreConfig::new(
            directory.path().join("hito.db"),
            music,
            HostDeviceId::new("lease-http-test-host").unwrap(),
        );
        owner.attach(&mut config);
        let services = hito_core::open(config).await.unwrap();
        let login = services.identity.setup(credentials()).await.unwrap();
        let actor = services.identity.authenticate(&login.token).await.unwrap();
        let library = services
            .catalog
            .libraries(&actor)
            .await
            .unwrap()
            .remove(0)
            .id;
        support::scan(&services, &actor, &library).await;
        let recording = services
            .catalog
            .recordings(&actor, Default::default())
            .await
            .unwrap()
            .items
            .remove(0)
            .id;
        let source = services
            .catalog
            .sources(&actor, &recording)
            .await
            .unwrap()
            .remove(0)
            .id;
        let app = hito_server::router(services.clone());
        Self {
            directory,
            owner,
            services,
            actor,
            app,
            token: login.token,
            recording,
            source,
            original,
        }
    }

    pub async fn begin(&self) -> Response {
        let body = serde_json::to_vec(&BeginPlaybackLease {
            recording_id: self.recording.clone(),
            source_id: self.source.clone(),
        })
        .unwrap();
        self.request("POST", "/api/v1/playback/leases", &[], Some(body))
            .await
    }

    pub async fn request(
        &self,
        method: &str,
        path: &str,
        headers: &[(&str, &str)],
        body: Option<Vec<u8>>,
    ) -> Response {
        self.request_as(Some(&self.token), method, path, headers, body)
            .await
    }

    pub async fn request_as(
        &self,
        token: Option<&str>,
        method: &str,
        path: &str,
        headers: &[(&str, &str)],
        body: Option<Vec<u8>>,
    ) -> Response {
        let mut request = Request::builder()
            .method(method)
            .uri(path)
            .header("x-hito-request", "1");
        if !headers
            .iter()
            .any(|(name, _)| name.eq_ignore_ascii_case("content-type"))
        {
            request = request.header(header::CONTENT_TYPE, "application/json");
        }
        if let Some(token) = token {
            request = request.header(header::AUTHORIZATION, format!("Bearer {token}"));
        }
        for (name, value) in headers {
            request = request.header(*name, *value);
        }
        self.app
            .clone()
            .oneshot(
                request
                    .body(body.map_or_else(Body::empty, Body::from))
                    .unwrap(),
            )
            .await
            .unwrap()
    }

    pub async fn close(self) {
        drop(self.app);
        self.services.playback.close().await.unwrap();
        self.services.replicas.close().await.unwrap();
        self.services.catalog_replication.close().await.unwrap();
        drop(self.services);
        let result = self.owner.retire(Duration::from_secs(5)).await;
        assert!(
            result.resources_retired && !result.deadline_exceeded,
            "SQLite resources must retire before releasing owned temporary storage"
        );
        assert_eq!(self.owner.observe().active_resources(), 0);
    }
}

pub fn credentials() -> Credentials {
    Credentials {
        username: "admin".into(),
        password: "owned HTTP lease test password".into(),
    }
}
