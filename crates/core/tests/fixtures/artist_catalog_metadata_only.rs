pub use content::ProviderServer;
use hito_core::test_support::content_service as content;

use super::support;
use hito_core::{
    Action, Actor, Browse, Error, ImportProviderPlaylist, InstallPlugin, ObservationOrigin,
    OfferPluginService, PlaylistImportReceipt, Services,
};
use hito_plugin_sdk::{
    Capability, Manifest, PROTOCOL_VERSION, PlaylistAttribution, PlaylistDetailRequest,
    PlaylistPage, ProviderPlaylist, ProviderPlaylistEntry, ProviderPlaylistItem, ProviderRecording,
    ProviderRelease,
};
use serde_json::Value;
use uuid::Uuid;

pub const CREDIT: &str = "Owned imported performer";
const ORIGIN: &str = "org.hito.spotify-fixture";
const FIRST: &str = "spotify:track:0000000000000000000001";
const SECOND: &str = "spotify:track:0000000000000000000002";

pub struct Imported {
    pub recording_id: String,
    pub other_recording_id: String,
    pub artist_id: String,
    pub observation_id: String,
    receipt: PlaylistImportReceipt,
}

impl Imported {
    pub async fn create(hito: &Services, actor: &Actor, origin: &ProviderServer) -> Self {
        let manifest = Manifest {
            id: ORIGIN.into(),
            name: "Owned playlist metadata".into(),
            protocol_version: PROTOCOL_VERSION,
            capabilities: [Capability::Playlists].into(),
            authorization: None,
            content_policy: None,
        };
        assert!(manifest.validate().is_ok());
        origin.reply("v1/manifest", value(manifest)).await;
        let detail = ProviderPlaylist {
            id: "spotify:playlist:owned-metadata-only".into(),
            name: "Owned metadata-only selection".into(),
            snapshot: "owned-metadata-only-v1".into(),
            total_count: 3,
            attribution: PlaylistAttribution {
                label: "Owned playlist fixture".into(),
            },
        };
        let expected_detail = detail.clone();
        let items = PlaylistPage {
            playlist_id: detail.id.clone(),
            snapshot: detail.snapshot.clone(),
            items: [FIRST, SECOND, FIRST]
                .into_iter()
                .enumerate()
                .map(|(index, id)| ProviderPlaylistEntry {
                    occurrence_id: Some(format!("owned-occurrence-{index}")),
                    item: ProviderPlaylistItem::Recording {
                        recording: recording(id),
                    },
                })
                .collect(),
            next_cursor: None,
        };
        assert!(detail.validate().is_ok() && items.validate(100).is_ok());
        origin.reply("v1/playlists/detail", value(detail)).await;
        origin.reply("v1/playlists/items", value(items)).await;
        let service = hito
            .plugins
            .offer_service(
                actor,
                OfferPluginService {
                    name: "Owned playlist metadata".into(),
                    base_url: origin.url.clone(),
                },
            )
            .await
            .unwrap();
        let installation = hito
            .plugins
            .install(
                actor,
                InstallPlugin {
                    accept_manifest_digest: None,
                    service_id: service.id,
                    service_revision: service.revision,
                    bearer_token: None,
                },
            )
            .await
            .unwrap();
        let receipt = hito
            .collection
            .import_provider_playlist(
                actor,
                &installation.id,
                ImportProviderPlaylist {
                    request_id: Uuid::new_v4(),
                    installation_revision: installation.revision,
                    playlist_reference: "https://provider.invalid/owned-metadata-only".into(),
                },
            )
            .await
            .unwrap();
        let detail_requests = origin
            .requests("v1/playlists/detail")
            .await
            .into_iter()
            .map(|(_, body)| {
                serde_json::from_value::<PlaylistDetailRequest>(body).unwrap_or_else(|_| {
                    panic!("Playlist detail requests must use the public protocol")
                })
            })
            .collect::<Vec<_>>();
        assert!(
            detail_requests.len() == 2,
            "Import must verify the final canonical playlist detail"
        );
        assert!(
            detail_requests
                .iter()
                .all(|request| request.validate().is_ok())
        );
        assert!(
            matches!(&detail_requests[0], PlaylistDetailRequest::Reference { reference }
                if reference == "https://provider.invalid/owned-metadata-only")
                && matches!(&detail_requests[1], PlaylistDetailRequest::Canonical { playlist_id }
                    if playlist_id == &expected_detail.id),
            "Import must resolve the supplied reference and then verify its exact canonical identity"
        );
        let playlist = hito
            .collection
            .playlist(actor, &receipt.playlist_id.to_string())
            .await
            .unwrap();
        assert!(playlist.playlist.kind == "local" && playlist.entries.len() == 3);
        let retained_origin = playlist
            .origin
            .as_ref()
            .expect("An editable provider copy must retain its origin");
        assert!(
            retained_origin.provider_playlist_id == expected_detail.id
                && retained_origin.snapshot == expected_detail.snapshot
                && retained_origin.attribution == expected_detail.attribution
                && playlist.playlist.name == expected_detail.name,
            "Import must retain the exact stable detail served for initial and final verification"
        );
        let recording_id = playlist.entries[0].recording_id.clone().unwrap();
        let other_recording_id = playlist.entries[1].recording_id.clone().unwrap();
        assert!(
            recording_id != other_recording_id
                && playlist.entries[2].recording_id.as_ref() == Some(&recording_id)
                && playlist.entries[0].entry_id != playlist.entries[2].entry_id
                && playlist
                    .entries
                    .iter()
                    .enumerate()
                    .all(|(index, entry)| entry.position == index as i64 && !entry.available),
            "Metadata-only import must preserve ordered duplicate occurrences and distinct recordings"
        );
        let recording_uuid = Uuid::parse_str(&recording_id).unwrap();
        support::synchronize(
            hito,
            actor,
            vec![
                Action::RecordingTitleSet {
                    recording_id: recording_uuid,
                    title: "Private imported title".into(),
                },
                Action::RecordingArtistSet {
                    recording_id: recording_uuid,
                    artist: CREDIT.into(),
                },
                Action::LikeSet {
                    recording_id: recording_uuid,
                    liked: true,
                },
                Action::ProgressSet {
                    recording_id: recording_uuid,
                    position_ms: 400,
                    speed: 1.25,
                },
            ],
        )
        .await;
        let metadata = hito
            .catalog
            .recording_metadata(actor, &recording_id, Default::default())
            .await
            .unwrap();
        assert!(metadata.title.personal.as_deref() == Some("Private imported title"));
        assert!(metadata.artist.personal.as_deref() == Some(CREDIT));
        let observation = metadata.selected_observation.unwrap();
        assert!(
            matches!(observation.origin, ObservationOrigin::ProviderResponse)
                && observation.source_id.is_none()
                && observation.plugin_installation_id.as_ref() == Some(&installation.id)
                && observation.provider_recording_id.as_deref() == Some(FIRST)
                && observation.provider == ORIGIN,
            "The current observation must retain its exact provider origin without an audio source"
        );
        let mut artists = hito
            .catalog
            .artists(actor, Browse::default())
            .await
            .unwrap();
        assert!(artists.total == 1 && artists.items[0].recording_count == 2);
        let artist_id = artists.items.remove(0).id;
        Self {
            recording_id,
            other_recording_id,
            artist_id,
            observation_id: observation.id,
            receipt,
        }
    }

    pub async fn snapshot(&self, hito: &Services, actor: &Actor) -> Value {
        let mut metadata = Vec::new();
        let mut sources = Vec::new();
        for recording in [&self.recording_id, &self.other_recording_id] {
            metadata.push(
                hito.catalog
                    .recording_metadata(actor, recording, Default::default())
                    .await
                    .unwrap(),
            );
            let current_sources = hito.catalog.sources(actor, recording).await.unwrap();
            assert!(
                current_sources.is_empty(),
                "Artist metadata must not create playable sources"
            );
            assert!(
                matches!(
                    hito.downloads.status(actor, recording).await,
                    Err(Error::NotFound)
                ),
                "Artist metadata must not create persistent downloads"
            );
            sources.push(current_sources);
        }
        let recordings = hito
            .catalog
            .recordings(actor, Default::default())
            .await
            .unwrap();
        assert!(
            recordings.total == 2
                && recordings
                    .items
                    .iter()
                    .all(|recording| !recording.available)
        );
        serde_json::json!({
            "receipt": self.receipt,
            "recordings": recordings,
            "metadata": metadata,
            "sources": sources,
            "artists": hito.catalog.artists(actor, Browse::default()).await.unwrap(),
            "releases": hito.catalog.releases(actor, Browse::default()).await.unwrap(),
            "playlist": hito.collection.playlist(actor, &self.receipt.playlist_id.to_string()).await.unwrap(),
            "activity": hito.collection.activity(actor).await.unwrap(),
            "progress": hito.collection.saved_playback(actor, &[
                Uuid::parse_str(&self.recording_id).unwrap(),
                Uuid::parse_str(&self.other_recording_id).unwrap(),
            ]).await.unwrap(),
            "events": hito.collection.events(actor, 0).await.unwrap(),
        })
    }
}

pub async fn request_counts(origin: &ProviderServer) -> Vec<(&'static str, usize)> {
    let mut counts = Vec::new();
    for path in [
        "v1/manifest",
        "v1/playlists/detail",
        "v1/playlists/items",
        "v1/search",
        "v1/recordings/detail",
        "v1/recordings/metadata",
        "v1/sources",
        "v1/audio",
    ] {
        counts.push((path, origin.requests(path).await.len()));
    }
    counts
}

fn recording(id: &str) -> ProviderRecording {
    ProviderRecording {
        id: id.into(),
        title: if id == FIRST {
            "First imported recording"
        } else {
            "Second imported recording"
        }
        .into(),
        artist: CREDIT.into(),
        duration_ms: Some(1000),
        release: Some(ProviderRelease {
            id: "spotify:album:owned".into(),
            title: "Owned imported album".into(),
            artist: Some(CREDIT.into()),
        }),
        disc_number: Some(1),
        track_number: Some(if id == FIRST { 1 } else { 2 }),
        revision: Some("owned-recording-v1".into()),
        attribution: None,
    }
}

fn value(input: impl serde::Serialize) -> Value {
    serde_json::to_value(input).unwrap()
}
