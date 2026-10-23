use super::{content_service as provider, support};
use crate::{
    Action, Actor, AdoptProviderRecording, CommunityRankingConfig, CoreConfig, Credentials,
    HostDeviceId, InstallPlugin, OfferPluginService, PluginInstallation, PreviewPluginInstallation,
    Recording, ResolveSource, Services, Source, SourceResolutionOutcome,
};
use serde_json::{Value, json};
use std::{future::pending, path::PathBuf};
use uuid::Uuid;

const TITLE: &str = "Owned private ranking tune";
const ARTIST: &str = "Owned private ranking performer";
const YOUTUBE_ID: &str = "RankVideo01";
const JAMENDO_ID: &str = "jamendo:track:v1:42";

pub struct Library {
    pub hito: Services,
    pub actor: Actor,
    pub recording: Recording,
    pub baseline: Vec<Source>,
    playlist: Uuid,
    original_path: PathBuf,
    pub original: Vec<u8>,
    original_metadata: std::fs::Metadata,
    _directory: tempfile::TempDir,
}

impl Library {
    pub async fn prepare(
        primary: &provider::ProviderServer,
        candidate: &provider::ProviderServer,
        ranking_url: &str,
    ) -> Self {
        let directory = tempfile::tempdir().unwrap();
        let music = directory.path().join("private-ranking-originals");
        std::fs::create_dir(&music).unwrap();
        let original_path = music.join("private-original.wav");
        let original = support::write_wav(&original_path, 41);
        let original_metadata = std::fs::metadata(&original_path).unwrap();
        let mut config = CoreConfig::new(
            directory.path().join("hito.db"),
            music,
            HostDeviceId::new("private-ranking-host").unwrap(),
        );
        config.community_rankings =
            Some(CommunityRankingConfig::new(ranking_url.to_owned()).unwrap());
        assert!(config.community.is_none());
        let hito = crate::open(config).await.unwrap();
        let session = hito.identity.setup(credentials("owner")).await.unwrap();
        let actor = hito.identity.authenticate(&session.token).await.unwrap();
        let youtube = connect(&hito, &actor, primary, "org.hito.youtube", &["sources"]).await;
        let jamendo = connect(
            &hito,
            &actor,
            candidate,
            "org.hito.jamendo",
            &["search", "sources"],
        )
        .await;
        primary
            .reply("v1/recordings/detail", details(YOUTUBE_ID))
            .await;
        primary.reply("v1/sources", sources("youtube", false)).await;
        let recording = hito
            .catalog
            .adopt_provider(
                &actor,
                &youtube.id,
                AdoptProviderRecording {
                    installation_revision: youtube.revision,
                    provider_recording_id: YOUTUBE_ID.into(),
                },
            )
            .await
            .unwrap();
        assert!(!recording.available);
        candidate
            .reply(
                "v1/search",
                json!({"items": [details(JAMENDO_ID)], "next_cursor": null}),
            )
            .await;
        candidate
            .reply("v1/recordings/detail", details(JAMENDO_ID))
            .await;
        candidate
            .reply("v1/sources", sources("jamendo", true))
            .await;
        let resolution = hito
            .catalog
            .resolve_source_until(
                &actor,
                &recording.id,
                ResolveSource {
                    request_id: Uuid::new_v4(),
                },
                pending(),
            )
            .await
            .unwrap();
        assert!(matches!(resolution.outcome, SourceResolutionOutcome::Ready));
        assert_eq!(candidate.requests("v1/search").await.len(), 1);
        assert_eq!(candidate.requests("v1/recordings/detail").await.len(), 1);
        assert_eq!(candidate.requests("v1/sources").await.len(), 1);
        primary.reply("v1/sources", sources("youtube", true)).await;
        hito.catalog
            .refresh_sources(&actor, &recording.id)
            .await
            .unwrap();
        let baseline = hito.catalog.sources(&actor, &recording.id).await.unwrap();
        assert!(baseline.len() == 2 && baseline.iter().all(|source| source.ready));
        let provenance = hito
            .catalog
            .recording_metadata(&actor, &recording.id, Default::default())
            .await
            .unwrap();
        assert!(provenance.observations.items.iter().any(|observation| {
            observation.plugin_installation_id.as_deref() == Some(jamendo.id.as_str())
                && observation.provider_recording_id.as_deref() == Some(JAMENDO_ID)
        }));
        let playlist = Uuid::new_v4();
        let recording_id = Uuid::parse_str(&recording.id).unwrap();
        support::synchronize(
            &hito,
            &actor,
            vec![
                Action::PlaylistCreate {
                    playlist_id: playlist,
                    name: "private-ranking-playlist".into(),
                },
                Action::EntryInsert {
                    playlist_id: playlist,
                    entry_id: Uuid::new_v4(),
                    recording_id,
                    position: 0,
                },
                Action::RecordingTitleSet {
                    recording_id,
                    title: "private-personal-ranking-title".into(),
                },
                Action::LikeSet {
                    recording_id,
                    liked: true,
                },
                Action::ProgressSet {
                    recording_id,
                    position_ms: 9000,
                    speed: 1.25,
                },
            ],
        )
        .await;
        Self {
            hito,
            actor,
            recording,
            baseline,
            playlist,
            original_path,
            original,
            original_metadata,
            _directory: directory,
        }
    }

    pub async fn private_state(&self) -> Value {
        json!({
            "catalog": self.hito.catalog.recordings(&self.actor, Default::default()).await.unwrap(),
            "metadata": self.hito.catalog.recording_metadata(&self.actor, &self.recording.id, Default::default()).await.unwrap(),
            "sources": self.hito.catalog.sources(&self.actor, &self.recording.id).await.unwrap(),
            "plugins": self.hito.plugins.settings(&self.actor).await.unwrap(),
            "playlist": self.hito.collection.playlist(&self.actor, &self.playlist.to_string()).await.unwrap(),
            "activity": self.hito.collection.activity(&self.actor).await.unwrap(),
            "events": self.hito.collection.events(&self.actor, 0).await.unwrap(),
            "position": self.hito.collection.saved_position(&self.actor, Uuid::parse_str(&self.recording.id).unwrap()).await.unwrap().map(|position| position.position_ms),
        })
    }

    pub fn assert_original(&self) {
        assert_eq!(std::fs::read(&self.original_path).unwrap(), self.original);
        let after_metadata = std::fs::metadata(&self.original_path).unwrap();
        assert_eq!(after_metadata.len(), self.original_metadata.len());
        assert_eq!(
            after_metadata.modified().unwrap(),
            self.original_metadata.modified().unwrap()
        );
    }
}

pub fn public_reference(source: &Source) -> Value {
    match source.label.as_str() {
        "private-youtube-label" => json!({"provider": "youtube", "recording_id": YOUTUBE_ID}),
        "private-jamendo-label" => json!({"provider": "jamendo", "recording_id": "42"}),
        _ => panic!("The owned fixture must expose only its two known source representations"),
    }
}

fn details(id: &str) -> Value {
    json!({"id": id, "title": TITLE, "artist": ARTIST, "duration_ms": 60000})
}

fn sources(provider: &str, ready: bool) -> Value {
    json!({"items": [{"id": format!("private-{provider}-audio?signature=canary"),
        "label": format!("private-{provider}-label"), "ready": ready}]})
}

async fn connect(
    hito: &Services,
    actor: &Actor,
    server: &provider::ProviderServer,
    id: &str,
    capabilities: &[&str],
) -> PluginInstallation {
    server
        .reply(
            "v1/manifest",
            json!({
                "id": id, "name": "Owned private ranking provider", "protocol_version": 1,
                "capabilities": capabilities,
                "content_policy": {"source_persistence": "playback_only"},
            }),
        )
        .await;
    let service = hito
        .plugins
        .offer_service(
            actor,
            OfferPluginService {
                name: "Owned private ranking provider".into(),
                base_url: server.url.clone(),
            },
        )
        .await
        .unwrap();
    let preview = hito
        .plugins
        .preview_installation(
            actor,
            PreviewPluginInstallation {
                service_id: service.id.clone(),
                service_revision: service.revision,
                bearer_token: Some("private-ranking-provider-credential".into()),
            },
        )
        .await
        .unwrap();
    hito.plugins
        .install(
            actor,
            InstallPlugin {
                service_id: service.id,
                service_revision: service.revision,
                accept_manifest_digest: Some(preview.digest),
                bearer_token: Some("private-ranking-provider-credential".into()),
            },
        )
        .await
        .unwrap()
}

fn credentials(username: &str) -> Credentials {
    Credentials {
        username: username.into(),
        password: "private-ranking-account-password".into(),
    }
}
