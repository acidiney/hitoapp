#[path = "provider_attachment.rs"]
mod provider;

use super::support;
use hito_core::{
    Actor, AttachReviewedProviderSource, ChooseSource, ImportProviderPlaylist,
    PlaylistImportReceipt, PluginInstallation, ProviderCandidate, Services,
};
use hito_plugin_sdk::ProviderRecording;
use serde_json::json;
use uuid::Uuid;

pub const PROVIDER: &str = "org.hito.metadata-origin-fixture";
pub const PLAYLIST: &str = "playlist/original";
pub const RECORDING: &str = "recording/existing";
pub const MISSING_RECORDING: &str = "recording/removed-upstream";
pub const RELEASE: &str = "release/verified";

pub struct Library {
    pub hito: Services,
    pub actor: Actor,
    pub origin: provider::ProviderServer,
    pub alternate: provider::ProviderServer,
    pub installation: PluginInstallation,
    pub first: PlaylistImportReceipt,
    pub second: PlaylistImportReceipt,
    _directory: tempfile::TempDir,
}

impl Library {
    pub async fn start() -> Self {
        let (directory, hito, actor, _shared, _music) =
            support::library("provider-metadata-backfill").await;
        let origin = provider::ProviderServer::start().await;
        let alternate = provider::ProviderServer::start().await;
        let installation =
            provider::connect(&hito, &actor, &origin, PROVIDER, &["playlists"]).await;
        let existing = json!({
            "id": RECORDING, "title": "Original provider title", "artist": "Original performer",
            "duration_ms": 90_000, "revision": "before-albums"
        });
        provider::playlist(
            &origin,
            PLAYLIST,
            "albumless-snapshot",
            vec![
                json!({"item": {"kind": "recording", "recording": existing}}),
                json!({"item": {"kind": "recording", "recording": {
                    "id": MISSING_RECORDING, "title": "Removed recording", "artist": "Retained performer",
                    "duration_ms": 120_000, "revision": "before-removal"
                }}}),
                json!({"item": {"kind": "recording", "recording": existing}}),
            ],
        )
        .await;
        let first = hito
            .collection
            .import_provider_playlist(
                &actor,
                &installation.id,
                import_request(&installation, Uuid::new_v4()),
            )
            .await
            .unwrap();
        let second = hito
            .collection
            .import_provider_playlist(
                &actor,
                &installation.id,
                import_request(&installation, Uuid::new_v4()),
            )
            .await
            .unwrap();
        Self {
            hito,
            actor,
            origin,
            alternate,
            installation,
            first,
            second,
            _directory: directory,
        }
    }

    pub async fn select_owned_alternate_source(&self, recording: &str) {
        let installation = provider::connect(
            &self.hito,
            &self.actor,
            &self.alternate,
            "org.hito.metadata-audio-fixture",
            &["sources"],
        )
        .await;
        self.alternate
            .reply(
                "v1/recordings/detail",
                json!({
                    "id": "alternate/existing", "title": "Alternate source title",
                    "artist": "Alternate source performer", "duration_ms": 90_000,
                    "revision": "alternate-one"
                }),
            )
            .await;
        self.alternate
            .reply(
                "v1/sources",
                json!({"items": [
                    {"id": "audio/existing", "label": "Owned ready source", "ready": true}
                ]}),
            )
            .await;
        let candidate = ProviderCandidate {
            installation_id: installation.id,
            installation_revision: installation.revision,
            provider_recording_id: "alternate/existing".into(),
        };
        let review = self
            .hito
            .catalog
            .review_provider_source(&self.actor, recording, candidate.clone())
            .await
            .unwrap();
        self.hito
            .catalog
            .attach_provider_source(
                &self.actor,
                recording,
                AttachReviewedProviderSource {
                    candidate,
                    review_stamp: review.review_stamp,
                },
            )
            .await
            .unwrap();
        let sources = self
            .hito
            .catalog
            .sources(&self.actor, recording)
            .await
            .unwrap();
        assert_eq!(sources.len(), 1);
        self.hito
            .catalog
            .choose_source(
                &self.actor,
                recording,
                ChooseSource {
                    request_id: Uuid::new_v4(),
                    source_id: Uuid::parse_str(&sources[0].id).unwrap(),
                },
            )
            .await
            .unwrap();
    }

    pub async fn publish_verified_album(&self) {
        provider::playlist(
            &self.origin,
            PLAYLIST,
            "verified-album-snapshot",
            vec![
                json!({"item": {"kind": "recording", "recording": {
                    "id": "recording/new-upstream", "title": "Removed recording",
                    "artist": "Retained performer", "duration_ms": 120_000,
                    "release": {"id": "release/unrelated", "title": "Unrelated upstream album"}
                }}}),
                json!({"item": {"kind": "recording", "recording": verified_metadata()}}),
                json!({"item": {"kind": "recording", "recording": verified_metadata()}}),
            ],
        )
        .await;
    }
}

pub fn import_request(
    installation: &PluginInstallation,
    request_id: Uuid,
) -> ImportProviderPlaylist {
    ImportProviderPlaylist {
        request_id,
        installation_revision: installation.revision,
        playlist_reference: "https://provider.invalid/original-copy".into(),
    }
}

pub fn verified_metadata() -> ProviderRecording {
    serde_json::from_value(json!({
        "id": RECORDING, "title": "Verified provider title", "artist": "Verified performer",
        "duration_ms": 91_000, "revision": "verified-album-one",
        "release": {"id": RELEASE, "title": "Verified provider album"},
        "disc_number": 1, "track_number": 4
    }))
    .unwrap()
}
