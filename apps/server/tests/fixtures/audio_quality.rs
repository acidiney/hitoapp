use axum::{
    Router,
    body::Body,
    http::{Request, header},
    response::Response,
};
use hito_core::{CoreConfig, Credentials, Search, Services, test_support::support};
use std::path::Path;
use tower::ServiceExt;

pub struct AudioFixture {
    pub directory: tempfile::TempDir,
    _tools: tempfile::TempDir,
    pub services: Services,
    pub app: Router,
    pub source: String,
    pub token: String,
    pub original: Vec<u8>,
}

impl AudioFixture {
    pub async fn start(
        extension: &str,
        write: impl FnOnce(&Path),
        configure: impl FnOnce(&mut CoreConfig, &Path),
    ) -> Self {
        let tools = tempfile::tempdir().unwrap();
        let (directory, services, actor, library, music) =
            support::library_configured("quality-http", |config| configure(config, tools.path()))
                .await;
        let path = music.join(format!("Private Quality Title.{extension}"));
        write(&path);
        let original = std::fs::read(path).unwrap();
        support::scan(&services, &actor, &library).await;
        let recording = services
            .catalog
            .recordings(&actor, Search::default())
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
        let session = services
            .identity
            .login(Credentials {
                username: "admin".into(),
                password: "indexing test password".into(),
            })
            .await
            .unwrap();
        let app = hito_server::router(services.clone());
        Self {
            directory,
            _tools: tools,
            services,
            app,
            source,
            token: session.token,
            original,
        }
    }

    pub async fn response(&self, method: &str, query: &str) -> Response {
        self.app
            .clone()
            .oneshot(
                Request::builder()
                    .method(method)
                    .uri(format!("/api/v1/sources/{}/audio{query}", self.source))
                    .header(header::AUTHORIZATION, format!("Bearer {}", self.token))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap()
    }
}

pub fn no_configuration(_: &mut CoreConfig, _: &Path) {}
