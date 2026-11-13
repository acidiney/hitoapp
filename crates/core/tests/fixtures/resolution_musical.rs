use hito_core::test_support::provider_attachment as provider;
pub use provider::ProviderServer;

use super::support;
use axum::http::{HeaderMap, HeaderValue, StatusCode};
use hito_core::{Action, Actor, ImportProviderPlaylist, PluginInstallation, Services};
use hito_plugin_sdk::{Capability, ProviderRecording, RecordingMetadata, SourcePage};
use serde_json::{Value, json};
use uuid::Uuid;

pub struct SavedRecording {
    pub hito: Services,
    pub actor: Actor,
    pub recording: String,
    pub audio_installation: PluginInstallation,
    pub original: Vec<u8>,
    playlist_id: String,
    before: Value,
    _directory: tempfile::TempDir,
}

impl SavedRecording {
    pub async fn prepare(spotify: &ProviderServer, youtube: &ProviderServer) -> Self {
        Self::prepare_named_target(spotify, youtube, "Owned tune", "Owned studio album").await
    }

    pub async fn prepare_named_target(
        spotify: &ProviderServer,
        youtube: &ProviderServer,
        title: &str,
        album: &str,
    ) -> Self {
        let (directory, hito, actor, _, _) = support::library("resolution-musical-evidence").await;
        let primary = provider::connect(
            &hito,
            &actor,
            spotify,
            "org.hito.spotify-fixture",
            &["playlists"],
        )
        .await;
        let audio = provider::connect(
            &hito,
            &actor,
            youtube,
            "org.hito.youtube-fixture",
            &["search", "sources", "metadata"],
        )
        .await;
        assert!(
            [
                Capability::Search,
                Capability::Sources,
                Capability::Metadata
            ]
            .iter()
            .all(|capability| audio.manifest.capabilities.contains(capability)),
            "the candidate installation must approve every requested capability"
        );
        let target = json!({
            "id": "spotify/owned-recording", "title": title,
            "artist": "Owned performer", "duration_ms": 1000, "revision": "spotify-r1",
            "release": {
                "id": "spotify/owned-album", "title": album,
                "artist": "Owned performer"
            }
        });
        provider::playlist(
            spotify,
            "spotify/owned-playlist",
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
                &primary.id,
                ImportProviderPlaylist {
                    request_id: Uuid::new_v4(),
                    installation_revision: primary.revision,
                    playlist_reference: "https://provider.invalid/owned-playlist".into(),
                },
            )
            .await
            .unwrap_or_else(|_| panic!("the owned playlist must import"));
        let playlist_id = imported.playlist_id.to_string();
        let playlist = hito
            .collection
            .playlist(&actor, &playlist_id)
            .await
            .unwrap_or_else(|_| panic!("the saved playlist must be readable"));
        assert!(
            playlist.entries.len() == 2,
            "both owned occurrences must import"
        );
        let recording = playlist.entries[0]
            .recording_id
            .as_ref()
            .unwrap_or_else(|| panic!("the saved occurrence must identify a recording"))
            .clone();
        assert!(
            playlist.entries[1].recording_id.as_deref() == Some(recording.as_str())
                && playlist.entries[0].entry_id != playlist.entries[1].entry_id
                && playlist.entries.iter().all(|entry| !entry.available),
            "duplicate occurrences must share one unavailable Hito recording"
        );
        let recording_id = Uuid::parse_str(&recording)
            .unwrap_or_else(|_| panic!("the saved recording must have a Hito identity"));
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
            ],
        )
        .await;
        assert!(
            hito.catalog
                .sources(&actor, &recording)
                .await
                .unwrap_or_else(|_| panic!("the saved source list must be readable"))
                .is_empty(),
            "the saved musical recording must start without an audio source"
        );
        let before = saved_state(&hito, &actor, &recording, &playlist_id).await;
        assert!(
            before["recordings"]["total"] == json!(1)
                && before["selected_observation"]["provider"] == "org.hito.spotify-fixture"
                && before["selected_observation"]["title"] == title
                && before["selected_observation"]["artist"] == "Owned performer"
                && before["selected_observation"]["duration_ms"] == json!(1000)
                && before["selected_observation"]["album"] == album
                && before["recordings"]["items"][0]["liked"] == json!(true)
                && before["saved_position"] == json!(400),
            "the saved target must retain known musical identity and collection state"
        );
        let original = prepare_evidence(youtube, &directory).await;
        Self {
            hito,
            actor,
            recording,
            audio_installation: audio,
            original,
            playlist_id,
            before,
            _directory: directory,
        }
    }

    pub async fn prepare_private_title(&mut self, title: &str) {
        support::synchronize(
            &self.hito,
            &self.actor,
            vec![Action::RecordingTitleSet {
                recording_id: Uuid::parse_str(&self.recording).unwrap(),
                title: title.into(),
            }],
        )
        .await;
        self.before =
            saved_state(&self.hito, &self.actor, &self.recording, &self.playlist_id).await;
        assert_eq!(self.before["title"]["value"], title);
    }

    pub fn assert_original_preserved(&self) {
        assert_eq!(
            std::fs::read(self._directory.path().join("owned-provider.wav")).unwrap(),
            self.original
        );
    }

    pub async fn assert_preserved(&self) {
        assert!(
            saved_state(&self.hito, &self.actor, &self.recording, &self.playlist_id).await
                == self.before,
            "automatic matching must preserve primary metadata, likes, progress and duplicate entries"
        );
    }
}

async fn prepare_evidence(youtube: &ProviderServer, directory: &tempfile::TempDir) -> Vec<u8> {
    let legacy = json!({
        "id": "youtube/owned-video", "title": "Owned performer - Owned tune (Official Audio)",
        "artist": "Owned performer", "duration_ms": 1000, "release": null,
        "disc_number": null, "track_number": null, "revision": "youtube-r1"
    });
    let metadata = json!({
        "recording": legacy, "musical_title": "Owned tune",
        "album_title": "Owned studio album", "source_ids": ["youtube/owned-audio"]
    });
    let sources = json!({"items": [{
        "id": "youtube/owned-audio", "label": "Owned audio", "ready": true
    }]});
    validate_evidence(&legacy, &metadata, &sources);
    youtube
        .reply(
            "v1/search",
            json!({"items": [{
        "id": "youtube/owned-video", "title": "Owned performer - Owned tune (Official Audio)",
        "artist": "channel:Owned channel", "duration_ms": null
    }], "next_cursor": null}),
        )
        .await;
    youtube.reply("v1/recordings/detail", legacy).await;
    youtube.reply("v1/recordings/metadata", metadata).await;
    youtube.reply("v1/sources", sources).await;
    let original = support::write_wav(&directory.path().join("owned-provider.wav"), 37);
    youtube
        .audio(
            StatusCode::OK,
            HeaderMap::from_iter([(
                axum::http::header::CONTENT_TYPE,
                HeaderValue::from_static("audio/wav"),
            )]),
            original.clone(),
        )
        .await;
    original
}

fn validate_evidence(legacy: &Value, metadata: &Value, sources: &Value) {
    let detail: ProviderRecording = serde_json::from_value(legacy.clone())
        .unwrap_or_else(|_| panic!("the legacy recording fixture must use the public wire type"));
    let evidence: RecordingMetadata = serde_json::from_value(metadata.clone())
        .unwrap_or_else(|_| panic!("the metadata fixture must use the public wire type"));
    let page: SourcePage = serde_json::from_value(sources.clone())
        .unwrap_or_else(|_| panic!("the source fixture must use the public wire type"));
    assert!(
        detail.validate().is_ok() && evidence.validate().is_ok() && page.validate().is_ok(),
        "the protocol evidence must be valid before resolution starts"
    );
    assert!(
        encoded(&metadata["recording"]) == encoded(legacy) && evidence.recording == detail,
        "metadata must preserve the exact legacy recording response"
    );
    assert!(
        evidence.source_ids == Some(page.items.iter().map(|source| source.id.clone()).collect()),
        "metadata source witnesses must cover the complete source page"
    );
}

async fn saved_state(hito: &Services, actor: &Actor, recording: &str, playlist: &str) -> Value {
    let metadata = hito
        .catalog
        .recording_metadata(actor, recording, Default::default())
        .await
        .unwrap_or_else(|_| panic!("the saved target metadata must remain readable"));
    let recordings = hito
        .catalog
        .recordings(actor, Default::default())
        .await
        .unwrap_or_else(|_| panic!("the saved recording catalog must remain readable"));
    let playlist = hito
        .collection
        .playlist(actor, playlist)
        .await
        .unwrap_or_else(|_| panic!("the duplicate playlist entries must remain readable"));
    let activity = hito
        .collection
        .activity(actor)
        .await
        .unwrap_or_else(|_| panic!("the saved listening progress must remain readable"));
    let recording_id = Uuid::parse_str(recording)
        .unwrap_or_else(|_| panic!("the target must retain its Hito identity"));
    let position = hito
        .collection
        .saved_position(actor, recording_id)
        .await
        .unwrap_or_else(|_| panic!("the saved playback position must remain readable"));
    json!({
        "recordings": without_availability(value(recordings), "items"),
        "selected_observation": metadata.selected_observation,
        "title": metadata.title, "artist": metadata.artist,
        "playlist": without_availability(value(playlist), "entries"),
        "activity": without_availability(value(activity), "history"),
        "saved_position": position.map(|position| position.position_ms),
    })
}

fn without_availability(mut page: Value, field: &str) -> Value {
    for item in page[field]
        .as_array_mut()
        .unwrap_or_else(|| panic!("the public saved-state page must contain its items"))
    {
        item.as_object_mut()
            .unwrap_or_else(|| panic!("the public saved-state item must be an object"))
            .remove("available");
        item.as_object_mut().unwrap().remove("plays_from_services");
    }
    page
}

fn value(input: impl serde::Serialize) -> Value {
    serde_json::to_value(input).unwrap_or_else(|_| panic!("the public saved state must serialize"))
}

pub fn encoded(input: &impl serde::Serialize) -> Vec<u8> {
    serde_json::to_vec(input)
        .unwrap_or_else(|_| panic!("the public protocol evidence must serialize"))
}
