#[path = "provider_attachment.rs"]
mod provider;
pub use provider::ProviderServer;

use crate::support;
use hito_core::{Action, Actor, ImportProviderPlaylist, Services};
use serde_json::{Value, json};
use std::path::PathBuf;
use uuid::Uuid;

pub struct SavedLibrary {
    pub hito: Services,
    pub actor: Actor,
    pub recording: String,
    playlist: String,
    original_path: PathBuf,
    original: Vec<u8>,
    before: Value,
    _directory: tempfile::TempDir,
}

impl SavedLibrary {
    pub async fn prepare(primary: &ProviderServer, audio: &ProviderServer) -> Self {
        let (directory, hito, actor, library, music) =
            support::library("resolution-unavailable").await;
        let original_path = music.join("Owned original.wav");
        let original = support::write_wav(&original_path, 29);
        support::scan(&hito, &actor, &library).await;
        let primary_connection = provider::connect(
            &hito,
            &actor,
            primary,
            "org.hito.primary-fixture",
            &["playlists"],
        )
        .await;
        provider::connect(
            &hito,
            &actor,
            audio,
            "org.hito.audio-fixture",
            &["search", "sources", "metadata"],
        )
        .await;
        let target = json!({
            "id": "primary/owned-recording", "title": "Owned tune",
            "artist": "Owned performer", "duration_ms": 1000,
            "release": {"id": "primary/owned-album", "title": "Owned album", "artist": "Owned performer"}
        });
        provider::playlist(
            primary,
            "primary/owned-playlist",
            "owned-snapshot",
            vec![
                json!({"occurrence_id": "first", "item": {"kind": "recording", "recording": target}}),
                json!({"occurrence_id": "second", "item": {"kind": "recording", "recording": target}}),
            ],
        )
        .await;
        let imported = hito
            .collection
            .import_provider_playlist(
                &actor,
                &primary_connection.id,
                ImportProviderPlaylist {
                    request_id: Uuid::new_v4(),
                    installation_revision: primary_connection.revision,
                    playlist_reference: "https://provider.invalid/owned-playlist".into(),
                },
            )
            .await
            .unwrap_or_else(|_| panic!("the owned playlist must import"));
        let playlist = imported.playlist_id.to_string();
        let entries = hito
            .collection
            .playlist(&actor, &playlist)
            .await
            .unwrap_or_else(|_| panic!("the owned playlist must remain readable"));
        assert!(
            entries.entries.len() == 2
                && entries.entries[0].recording_id == entries.entries[1].recording_id
                && entries.entries[0].entry_id != entries.entries[1].entry_id
                && entries.entries.iter().all(|entry| !entry.available),
            "the import must retain two distinct unavailable occurrences"
        );
        let recording = entries.entries[0]
            .recording_id
            .clone()
            .unwrap_or_else(|| panic!("the imported target must identify a recording"));
        let recording_id = Uuid::parse_str(&recording)
            .unwrap_or_else(|_| panic!("the target must have a Hito recording identity"));
        support::synchronize(
            &hito,
            &actor,
            vec![
                Action::LikeSet {
                    recording_id,
                    liked: true,
                },
                Action::ProgressSet {
                    recording_id,
                    position_ms: 400,
                    speed: 1.25,
                },
                Action::RecordingTitleSet {
                    recording_id,
                    title: "Private title override".into(),
                },
            ],
        )
        .await;
        let detail = json!({
            "id": "audio/owned-recording", "title": "Owned performer - Owned tune (Official Audio)",
            "artist": "Owned performer", "duration_ms": 1000,
            "release": null, "disc_number": null, "track_number": null, "revision": "owned-r1"
        });
        audio
            .reply("v1/search", json!({"items": [detail], "next_cursor": null}))
            .await;
        audio.reply("v1/recordings/detail", detail.clone()).await;
        audio
            .reply(
                "v1/sources",
                json!({"items": [{
                    "id": "audio/owned-source", "label": "Owned audio", "ready": true
                }]}),
            )
            .await;
        audio.reply("v1/recordings/metadata", json!({
            "recording": detail, "musical_title": "Owned tune", "album_title": "Owned album",
            "source_ids": ["audio/owned-source"]
        })).await;
        let before = snapshot(&hito, &actor, &recording, &playlist).await;
        assert!(
            before["recordings"]["total"] == 2
                && before["sources"].as_array().is_some_and(Vec::is_empty)
                && before["metadata"]["title"]["value"] == "Private title override"
                && before["position"] == 400,
            "the baseline must retain the local original, private edit and imported progress"
        );
        Self {
            hito,
            actor,
            recording,
            playlist,
            original_path,
            original,
            before,
            _directory: directory,
        }
    }

    pub async fn assert_preserved(&self) {
        let after = snapshot(&self.hito, &self.actor, &self.recording, &self.playlist).await;
        assert!(
            after == self.before,
            "failed source discovery and receipt replay must preserve exact private library state"
        );
        let bytes = std::fs::read(&self.original_path)
            .unwrap_or_else(|_| panic!("the user-owned original must remain readable"));
        assert!(
            bytes == self.original,
            "failed source discovery must preserve the indexed original bytes"
        );
        assert!(
            matches!(
                self.hito
                    .downloads
                    .status(&self.actor, &self.recording)
                    .await,
                Err(hito_core::Error::NotFound)
            ),
            "source discovery must not create a persistent download"
        );
    }
}

async fn snapshot(hito: &Services, actor: &Actor, recording: &str, playlist: &str) -> Value {
    let metadata = hito
        .catalog
        .recording_metadata(actor, recording, Default::default())
        .await
        .unwrap_or_else(|_| panic!("the target metadata must remain readable"));
    let recordings = hito
        .catalog
        .recordings(actor, Default::default())
        .await
        .unwrap_or_else(|_| panic!("the public recording page must remain readable"));
    let sources = hito
        .catalog
        .sources(actor, recording)
        .await
        .unwrap_or_else(|_| panic!("the target source list must remain readable"));
    let playlist = hito
        .collection
        .playlist(actor, playlist)
        .await
        .unwrap_or_else(|_| panic!("the exact duplicate playlist must remain readable"));
    let activity = hito
        .collection
        .activity(actor)
        .await
        .unwrap_or_else(|_| panic!("the private listening activity must remain readable"));
    let recording_id = Uuid::parse_str(recording)
        .unwrap_or_else(|_| panic!("the target must retain its Hito recording identity"));
    let position = hito
        .collection
        .saved_position(actor, recording_id)
        .await
        .unwrap_or_else(|_| panic!("the private progress must remain readable"));
    json!({
        "metadata": metadata, "recordings": recordings, "sources": sources,
        "playlist": playlist, "activity": activity,
        "position": position.map(|position| position.position_ms)
    })
}
