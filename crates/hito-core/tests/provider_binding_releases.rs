use hito_core::test_support::content_service as fixture;
use hito_core::test_support::support;

use hito_core::{
    Action, AdoptProviderRecording, AttachReviewedProviderSource, InstallPlugin,
    OfferPluginService, ProviderCandidate,
};
use serde_json::json;
use uuid::Uuid;

#[tokio::test]
async fn source_only_attachment_and_readoption_preserve_primary_release_metadata() {
    let server = fixture::ProviderServer::start().await;
    let (_directory, hito, actor, _library, _music) = support::library("binding-releases").await;
    let service = hito
        .plugins
        .offer_service(
            &actor,
            OfferPluginService {
                name: "Owned release fixture".into(),
                base_url: server.url.clone(),
            },
        )
        .await
        .unwrap();
    let installed = hito
        .plugins
        .install(
            &actor,
            InstallPlugin {
                accept_manifest_digest: None,
                service_id: service.id,
                service_revision: service.revision,
                bearer_token: None,
            },
        )
        .await
        .unwrap();
    server.reply("v1/recordings/detail", json!({
        "id": "primary", "title": "Primary track", "artist": "Primary artist", "duration_ms": 90_000,
        "release": {"id": "album", "title": "Primary album", "artist": "Primary album artist"},
        "disc_number": 1, "track_number": 1, "revision": "primary-r1"
    })).await;
    server
        .reply(
            "v1/sources",
            json!({"items": [{
                "id": "primary-audio", "label": "Primary source", "ready": true
            }]}),
        )
        .await;
    let primary = hito
        .catalog
        .adopt_provider(
            &actor,
            &installed.id,
            AdoptProviderRecording {
                installation_revision: installed.revision,
                provider_recording_id: "primary".into(),
            },
        )
        .await
        .unwrap();
    let release_id = primary.release_id.clone().unwrap();
    let recording_id = Uuid::parse_str(&primary.id).unwrap();
    let playlist_id = Uuid::new_v4();
    support::synchronize(
        &hito,
        &actor,
        vec![
            Action::PlaylistCreate {
                playlist_id,
                name: "Primary entries".into(),
            },
            Action::EntryInsert {
                playlist_id,
                entry_id: Uuid::new_v4(),
                recording_id,
                position: 0,
            },
            Action::EntryInsert {
                playlist_id,
                entry_id: Uuid::new_v4(),
                recording_id,
                position: 1,
            },
        ],
    )
    .await;
    let playlist = playlist_id.to_string();
    let entries = serde_json::to_value(
        hito.collection
            .playlist(&actor, &playlist)
            .await
            .unwrap()
            .entries,
    )
    .unwrap();
    let metadata = hito
        .catalog
        .recording_metadata(&actor, &primary.id, Default::default())
        .await
        .unwrap();
    let selected = serde_json::to_value(metadata.selected_observation).unwrap();
    let original_release = hito
        .catalog
        .release(&actor, &release_id, Default::default())
        .await
        .unwrap();
    assert_eq!(original_release.tracks.items.len(), 1);
    let primary_entry = original_release.tracks.items[0].entry_id.clone();
    let primary_appearance = serde_json::to_value(&original_release.tracks.items[0]).unwrap();
    assert_eq!(original_release.release.title, "Primary album");
    assert_eq!(
        original_release.release.artist.as_deref(),
        Some("Primary album artist")
    );

    server.reply("v1/recordings/detail", json!({
        "id": "alternate", "title": "Alternate track", "artist": "Alternate artist", "duration_ms": 95_000,
        "release": {"id": "album", "title": "Conflicting album", "artist": "Conflicting album artist"},
        "disc_number": 1, "track_number": 2, "revision": "alternate-r1"
    })).await;
    server
        .reply(
            "v1/sources",
            json!({"items": [{
                "id": "alternate-audio", "label": "Alternate source", "ready": true
            }]}),
        )
        .await;
    let candidate = ProviderCandidate {
        installation_id: installed.id.clone(),
        installation_revision: installed.revision,
        provider_recording_id: "alternate".into(),
    };
    let review = hito
        .catalog
        .review_provider_source(&actor, &primary.id, candidate.clone())
        .await
        .unwrap();
    let mut attached = hito
        .catalog
        .attach_provider_source(
            &actor,
            &primary.id,
            AttachReviewedProviderSource {
                candidate,
                review_stamp: review.review_stamp,
            },
        )
        .await
        .unwrap();
    for re_adopt in [false, true] {
        if re_adopt {
            attached = hito
                .catalog
                .adopt_provider(
                    &actor,
                    &installed.id,
                    AdoptProviderRecording {
                        installation_revision: installed.revision,
                        provider_recording_id: "alternate".into(),
                    },
                )
                .await
                .unwrap();
        }
        assert_eq!(attached.id, primary.id);
        assert_eq!(attached.inherited_title, "Primary track");
        assert_eq!(attached.inherited_artist, "Primary artist");
        assert_eq!(attached.release_id.as_deref(), Some(release_id.as_str()));
        let release_metadata = hito
            .catalog
            .release_metadata(&actor, &release_id, Default::default())
            .await
            .unwrap();
        assert_eq!(
            release_metadata.title.inherited, "Primary album",
            "a source-only observation must not publish inherited album metadata"
        );
        assert_eq!(
            release_metadata.artist.inherited.as_deref(),
            Some("Primary album artist")
        );
        assert_eq!(attached.album.as_deref(), Some("Primary album"));
        let recordings = hito
            .catalog
            .recordings(&actor, Default::default())
            .await
            .unwrap();
        assert_eq!(recordings.total, 1);
        assert_eq!(recordings.items[0].id, primary.id);
        assert_eq!(recordings.items[0].album.as_deref(), Some("Primary album"));
        let after = hito
            .catalog
            .recording_metadata(&actor, &primary.id, Default::default())
            .await
            .unwrap();
        assert_eq!(
            serde_json::to_value(after.selected_observation).unwrap(),
            selected
        );
        assert_eq!(after.observations.items.len(), 2);
        let observed = after
            .observations
            .items
            .iter()
            .find(|observation| observation.provider_recording_id.as_deref() == Some("alternate"))
            .expect("the alternate source retains its own observation");
        assert_eq!(observed.title, "Alternate track");
        assert_eq!(observed.artist, "Alternate artist");
        assert_eq!(observed.album.as_deref(), Some("Conflicting album"));
        assert_eq!(
            observed.album_artist.as_deref(),
            Some("Conflicting album artist")
        );
        assert_eq!(
            observed.plugin_installation_id.as_deref(),
            Some(installed.id.as_str())
        );
        let sources = hito.catalog.sources(&actor, &primary.id).await.unwrap();
        assert_eq!(sources.len(), 2);
        assert!(
            sources
                .iter()
                .all(|source| source.ready && source.recording_id == primary.id)
        );
        assert!(sources.iter().any(|source| source.id == primary_entry));
        let source = sources
            .iter()
            .find(|source| source.id != primary_entry)
            .unwrap();
        let observed_release = hito
            .catalog
            .release(
                &actor,
                observed.release_id.as_deref().unwrap(),
                Default::default(),
            )
            .await
            .unwrap();
        let appearance = observed_release
            .tracks
            .items
            .iter()
            .find(|track| track.entry_id == source.id)
            .expect("the alternate source exposes its own observed recording metadata");
        assert_eq!(appearance.recording.id, primary.id);
        assert_eq!(appearance.recording.inherited_title, "Alternate track");
        assert_eq!(appearance.recording.inherited_artist, "Alternate artist");
        assert_eq!(appearance.recording.duration_ms, 95_000);
        let detail = hito
            .catalog
            .release(&actor, &release_id, Default::default())
            .await
            .unwrap();
        assert_eq!(detail.release.title, "Primary album");
        assert_eq!(
            detail.release.artist.as_deref(),
            Some("Primary album artist")
        );
        let appearance = detail
            .tracks
            .items
            .iter()
            .find(|track| track.entry_id == primary_entry)
            .unwrap();
        assert_eq!(
            serde_json::to_value(appearance).unwrap(),
            primary_appearance
        );
        let releases = hito
            .catalog
            .releases(&actor, Default::default())
            .await
            .unwrap();
        let release = releases
            .items
            .iter()
            .find(|release| release.id == release_id)
            .unwrap();
        assert_eq!(release.title, "Primary album");
        assert_eq!(release.artist.as_deref(), Some("Primary album artist"));
        assert_eq!(
            serde_json::to_value(
                hito.collection
                    .playlist(&actor, &playlist)
                    .await
                    .unwrap()
                    .entries
            )
            .unwrap(),
            entries
        );
    }
}
