use hito_core::test_support::{content_service::ProviderServer, hang_guard::hang_guard, support};
use hito_core::{
    Actor, AdoptProviderRecording, AutomaticCopyBudget, Credentials, DownloadRequest,
    DownloadState, Downloads, InstallPlugin, OfferPluginService, PluginInstallation,
    PreviewPluginInstallation, Services, UpcomingRecordings,
};
use serde_json::json;
use sqlx::{Connection, SqliteConnection, sqlite::SqliteConnectOptions};
use std::{future::pending, path::PathBuf, time::Duration};
use uuid::Uuid;

pub const HOST: &str = "automatic-copies";

pub struct Copies {
    pub directory: tempfile::TempDir,
    pub music: PathBuf,
    pub server: ProviderServer,
    pub hito: Services,
    pub actor: Actor,
    pub original: Vec<u8>,
    pub installation: PluginInstallation,
}

impl Copies {
    pub async fn start() -> Self {
        Self::with_policy(None).await
    }

    pub async fn with_policy(policy: Option<&str>) -> Self {
        hang_guard("the automatic copy fixture must open", async {
            let server = ProviderServer::start().await;
            let mut manifest = json!({
                "id": "org.hito.copy-fixture",
                "name": "Copy fixture",
                "protocol_version": 1,
                "capabilities": ["search", "sources"]
            });
            if let Some(policy) = policy {
                manifest["content_policy"] = json!({"source_persistence": policy});
            }
            server.reply("v1/manifest", manifest).await;
            let (directory, hito, actor, _library, music) = support::library(HOST).await;
            let original = support::write_wav(&directory.path().join("provider.wav"), 7);
            server
                .audio(
                    axum::http::StatusCode::OK,
                    hito_core::test_support::provider_audio::headers(),
                    original.clone(),
                )
                .await;
            let service = hito
                .plugins
                .offer_service(
                    &actor,
                    OfferPluginService {
                        name: "Copy fixture".into(),
                        base_url: server.url.clone(),
                    },
                )
                .await
                .unwrap();
            let preview = hito
                .plugins
                .preview_installation(
                    &actor,
                    PreviewPluginInstallation {
                        service_id: service.id.clone(),
                        service_revision: service.revision,
                        bearer_token: None,
                    },
                )
                .await
                .unwrap();
            let installation = hito
                .plugins
                .install(
                    &actor,
                    InstallPlugin {
                        accept_manifest_digest: Some(preview.digest),
                        service_id: service.id,
                        service_revision: service.revision,
                        bearer_token: None,
                    },
                )
                .await
                .unwrap();
            Self {
                directory,
                music,
                server,
                hito,
                actor,
                original,
                installation,
            }
        })
        .await
    }

    pub async fn recording(&self, name: &str) -> String {
        hang_guard("a provider recording must be adopted", async {
            self.server
                .recording_reply(
                    "v1/recordings/detail",
                    name,
                    json!({"id": name, "title": "Copy fixture track", "artist": "Copy fixture artist"}),
                )
                .await;
            self.server
                .recording_reply(
                    "v1/sources",
                    name,
                    json!({"items": [{"id": format!("audio/{name}"), "label": "Original", "ready": true}]}),
                )
                .await;
            let installation = &self.installation;
            self.hito
                .catalog
                .adopt_provider(
                    &self.actor,
                    &installation.id,
                    AdoptProviderRecording {
                        installation_revision: installation.revision,
                        provider_recording_id: name.into(),
                    },
                )
                .await
                .unwrap()
                .id
        })
        .await
    }

    pub async fn announce(&self, actor: &Actor, recordings: &[&str]) {
        self.hito
            .downloads
            .announce_upcoming(
                actor,
                UpcomingRecordings {
                    client: Some("fixture-player".into()),
                    recording_ids: recordings.iter().map(|id| (*id).to_owned()).collect(),
                },
            )
            .await
            .unwrap();
    }

    pub async fn drain(&self) -> usize {
        drain(&self.hito).await
    }

    pub async fn copy_source(&self, recording: &str) -> Option<String> {
        self.hito
            .catalog
            .sources(&self.actor, recording)
            .await
            .unwrap()
            .into_iter()
            .find(|source| source.provider == "download" && source.ready)
            .map(|source| source.id)
    }

    pub async fn plugin_source(&self, recording: &str) -> String {
        self.hito
            .catalog
            .sources(&self.actor, recording)
            .await
            .unwrap()
            .into_iter()
            .find(|source| source.provider == "plugin")
            .unwrap()
            .id
    }

    pub async fn download(&self, recording: &str) {
        self.hito
            .downloads
            .create(
                &self.actor,
                recording,
                DownloadRequest {
                    request_id: Uuid::new_v4(),
                },
            )
            .await
            .unwrap();
        while tokio::time::timeout(
            Duration::from_secs(20),
            self.hito.downloads.run_next_job_until(pending()),
        )
        .await
        .expect("the explicit download must finish")
        .unwrap()
        {}
        assert!(matches!(
            self.hito
                .downloads
                .status(&self.actor, recording)
                .await
                .unwrap()
                .status,
            DownloadState::Ready
        ));
    }

    pub async fn budget(&self, bytes: u64) {
        self.hito
            .downloads
            .set_automatic_copy_budget(
                &self.actor,
                &self.actor.user().id,
                AutomaticCopyBudget {
                    budget_bytes: bytes,
                },
            )
            .await
            .unwrap();
    }

    pub async fn member(&self) -> Actor {
        let credentials = || Credentials {
            username: "member".into(),
            password: "automatic copy member password".into(),
        };
        self.hito
            .identity
            .create_user(&self.actor, credentials())
            .await
            .unwrap();
        let session = self.hito.identity.login(credentials()).await.unwrap();
        self.hito
            .identity
            .authenticate(&session.token)
            .await
            .unwrap()
    }

    pub async fn sql(&self, statement: &str) -> u64 {
        sql(&self.directory, statement).await
    }

    pub async fn count(&self, query: &str) -> i64 {
        let mut connection = connect(&self.directory).await;
        sqlx::query_scalar(query)
            .fetch_one(&mut connection)
            .await
            .unwrap()
    }

    pub fn managed_files(&self) -> Vec<String> {
        managed_files(&self.directory)
    }
}

pub async fn drain(hito: &Services) -> usize {
    drain_downloads(&hito.downloads).await
}

pub async fn drain_owned(downloads: Downloads) -> usize {
    drain_downloads(&downloads).await
}

pub async fn drain_downloads(downloads: &Downloads) -> usize {
    let mut runs = 0;
    while tokio::time::timeout(
        Duration::from_secs(20),
        downloads.run_next_copy_until(pending()),
    )
    .await
    .expect("automatic copy work must finish")
    .unwrap()
    {
        runs += 1;
        assert!(runs < 32, "automatic copy work must settle");
    }
    runs
}

pub async fn connect(directory: &tempfile::TempDir) -> SqliteConnection {
    SqliteConnection::connect_with(
        &SqliteConnectOptions::new().filename(directory.path().join("hito.db")),
    )
    .await
    .unwrap()
}

pub async fn sql(directory: &tempfile::TempDir, statement: &str) -> u64 {
    let mut connection = connect(directory).await;
    sqlx::query(statement)
        .execute(&mut connection)
        .await
        .unwrap()
        .rows_affected()
}

pub fn managed_files(directory: &tempfile::TempDir) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(directory.path().join("hito.downloads"))
        .map(|entries| {
            entries
                .map(|entry| entry.unwrap().file_name().into_string().unwrap())
                .collect()
        })
        .unwrap_or_default();
    names.sort();
    names
}
