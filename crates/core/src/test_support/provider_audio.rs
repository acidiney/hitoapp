use super::{content_service::ProviderServer, playlist_service, support};
use crate::{
    Actor, AdoptProviderRecording, AttachReviewedProviderSource, DownloadRequest, DownloadState,
    InstallPlugin, OfferPluginService, PluginInstallation, ProviderCandidate, Services, Source,
};
use std::future::pending;
use uuid::Uuid;

pub struct Alternate {
    pub server: ProviderServer,
    pub saved_copy: String,
    pub second: String,
}

pub struct AudioLibrary {
    pub directory: tempfile::TempDir,
    pub music: std::path::PathBuf,
    pub server: ProviderServer,
    pub hito: Services,
    pub actor: Actor,
    pub source: Source,
    pub original: Vec<u8>,
    pub installation: PluginInstallation,
}
impl AudioLibrary {
    pub async fn start() -> Self {
        Self::start_paced(crate::StreamPace::DEFAULT).await
    }
    pub async fn start_paced(pace: crate::StreamPace) -> Self {
        let server = ProviderServer::start().await;
        let (directory, hito, actor, _library, music) =
            support::library_configured("provider-audio", |config| config.stream_pace = pace).await;
        let original = support::write_wav(&music.join("original.wav"), 10);
        let service = hito
            .plugins
            .offer_service(
                &actor,
                OfferPluginService {
                    name: "Fixture".into(),
                    base_url: server.url.clone(),
                },
            )
            .await
            .unwrap();
        let installation = hito
            .plugins
            .install(
                &actor,
                InstallPlugin {
                    accept_manifest_digest: None,
                    service_id: service.id,
                    service_revision: 1,
                    bearer_token: Some("private-audio-token".into()),
                },
            )
            .await
            .unwrap();
        server.reply("v1/recordings/detail", serde_json::json!({"id":"recording","title":"Fixture track","artist":"Fixture artist"})).await;
        server.reply("v1/sources", serde_json::json!({"items":[{"id":"audio:/1?private=value&other=bad","label":"Original","ready":true}]})).await;
        let recording = hito
            .catalog
            .adopt_provider(
                &actor,
                &installation.id,
                AdoptProviderRecording {
                    installation_revision: 1,
                    provider_recording_id: "recording".into(),
                },
            )
            .await
            .unwrap();
        let source = hito
            .catalog
            .sources(&actor, &recording.id)
            .await
            .unwrap()
            .remove(0);
        Self {
            directory,
            music,
            server,
            hito,
            actor,
            source,
            original,
            installation,
        }
    }
    pub async fn save_copy_and_attach_alternate(&self) -> Alternate {
        self.ready().await;
        self.hito
            .downloads
            .create(
                &self.actor,
                &self.source.recording_id,
                DownloadRequest {
                    request_id: Uuid::new_v4(),
                },
            )
            .await
            .unwrap();
        assert!(
            self.hito
                .downloads
                .run_next_job_until(pending())
                .await
                .unwrap()
        );
        let saved = self
            .hito
            .downloads
            .status(&self.actor, &self.source.recording_id)
            .await
            .unwrap();
        assert!(matches!(saved.status, DownloadState::Ready));
        let server = ProviderServer::start().await;
        let installation =
            playlist_service::connect(&self.hito, &self.actor, &server, &["search", "sources"])
                .await;
        server
            .reply(
                "v1/recordings/detail",
                serde_json::json!({"id": "alternate/A", "title": "Alternate", "artist": "Fixture artist", "duration_ms": 90_000, "revision": "r1"}),
            )
            .await;
        server
            .reply(
                "v1/sources",
                serde_json::json!({"items": [{"id": "audio/second", "label": "Second", "ready": true}]}),
            )
            .await;
        let candidate = ProviderCandidate {
            installation_id: installation.id.clone(),
            installation_revision: installation.revision,
            provider_recording_id: "alternate/A".into(),
        };
        let review = self
            .hito
            .catalog
            .review_provider_source(&self.actor, &self.source.recording_id, candidate.clone())
            .await
            .unwrap();
        self.hito
            .catalog
            .attach_provider_source(
                &self.actor,
                &self.source.recording_id,
                AttachReviewedProviderSource {
                    candidate,
                    review_stamp: review.review_stamp,
                },
            )
            .await
            .unwrap();
        let second = self
            .hito
            .catalog
            .sources(&self.actor, &self.source.recording_id)
            .await
            .unwrap()
            .into_iter()
            .find(|source| source.label == "Second")
            .unwrap()
            .id;
        Alternate {
            server,
            saved_copy: saved.source_id.unwrap(),
            second,
        }
    }
    pub async fn ready(&self) {
        self.server
            .audio(axum::http::StatusCode::OK, headers(), self.original.clone())
            .await;
    }
}
pub fn headers() -> axum::http::HeaderMap {
    axum::http::HeaderMap::from_iter([(
        "content-type".parse().unwrap(),
        "audio/wav".parse().unwrap(),
    )])
}
