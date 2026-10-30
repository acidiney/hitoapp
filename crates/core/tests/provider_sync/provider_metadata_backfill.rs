#[path = "../fixtures/provider_metadata_backfill.rs"]
mod fixture;
use hito_core::test_support::support;

use hito_core::{Action, Activity, Error, PlaylistDetail, RefreshImportedPlaylistMetadata};
use serde_json::{Value, json};
use uuid::Uuid;

#[tokio::test]
async fn explicit_album_repair_preserves_independent_copies_and_publishes_only_existing_metadata() {
    let f = fixture::Library::start().await;
    let first_id = f.first.playlist_id.to_string();
    let second_id = f.second.playlist_id.to_string();
    let first = f
        .hito
        .collection
        .playlist(&f.actor, &first_id)
        .await
        .unwrap();
    let second = f
        .hito
        .collection
        .playlist(&f.actor, &second_id)
        .await
        .unwrap();
    let recording = first.entries[0].recording_id.clone().unwrap();
    let missing = first.entries[1].recording_id.clone().unwrap();
    let recording_id = Uuid::parse_str(&recording).unwrap();
    assert_eq!(
        first.entries[2].recording_id.as_deref(),
        Some(recording.as_str())
    );
    assert_eq!(
        second.entries[0].recording_id.as_deref(),
        Some(recording.as_str())
    );
    assert_ne!(first.entries[0].entry_id, first.entries[2].entry_id);
    assert_ne!(first.entries[0].entry_id, second.entries[0].entry_id);
    assert_eq!(
        f.hito
            .catalog
            .releases(&f.actor, Default::default())
            .await
            .unwrap()
            .total,
        0
    );

    f.select_owned_alternate_source(&recording).await;
    support::synchronize(
        &f.hito,
        &f.actor,
        vec![
            Action::RecordingTitleSet {
                recording_id,
                title: "My private title".into(),
            },
            Action::RecordingArtistSet {
                recording_id,
                artist: "My private performer".into(),
            },
            Action::LikeSet {
                recording_id,
                liked: true,
            },
            Action::ProgressSet {
                recording_id,
                position_ms: 4_200,
                speed: 1.25,
            },
            Action::PlaylistRename {
                playlist_id: f.first.playlist_id,
                name: "My first copy".into(),
            },
            Action::EntryMove {
                playlist_id: f.first.playlist_id,
                entry_id: Uuid::parse_str(&first.entries[0].entry_id).unwrap(),
                position: 10,
            },
            Action::PlaylistRename {
                playlist_id: f.second.playlist_id,
                name: "My second copy".into(),
            },
            Action::EntryRemove {
                playlist_id: f.second.playlist_id,
                entry_id: Uuid::parse_str(&second.entries[0].entry_id).unwrap(),
            },
            Action::EntryInsert {
                playlist_id: f.second.playlist_id,
                entry_id: Uuid::new_v4(),
                recording_id,
                position: 0,
            },
        ],
    )
    .await;

    let first_before = f
        .hito
        .collection
        .playlist(&f.actor, &first_id)
        .await
        .unwrap();
    let second_before = f
        .hito
        .collection
        .playlist(&f.actor, &second_id)
        .await
        .unwrap();
    let metadata_before = f
        .hito
        .catalog
        .recording_metadata(&f.actor, &recording, Default::default())
        .await
        .unwrap();
    let missing_before = f
        .hito
        .catalog
        .recording_metadata(&f.actor, &missing, Default::default())
        .await
        .unwrap();
    let sources_before =
        serde_json::to_vec(&f.hito.catalog.sources(&f.actor, &recording).await.unwrap()).unwrap();
    let choice_before = serde_json::to_value(
        f.hito
            .catalog
            .source_choice(&f.actor, &recording)
            .await
            .unwrap(),
    )
    .unwrap();
    let progress_before = progress_state(&f.hito.collection.activity(&f.actor).await.unwrap());
    let first_receipt_before = serde_json::to_vec(&f.first).unwrap();
    let second_receipt_before = serde_json::to_vec(&f.second).unwrap();
    let events_before = f.hito.collection.events(&f.actor, 0).await.unwrap();
    let old_event_bytes = serde_json::to_vec(&events_before.operations).unwrap();
    let detail_requests_before = f.origin.requests("v1/playlists/detail").await.len();
    let item_requests_before = f.origin.requests("v1/playlists/items").await.len();
    let alternate_details_before = f.alternate.requests("v1/recordings/detail").await.len();
    let alternate_sources_before = f.alternate.requests("v1/sources").await.len();
    assert_eq!(choice_before["mode"], "manual");
    assert!(
        metadata_before
            .selected_observation
            .as_ref()
            .unwrap()
            .release_id
            .is_none()
    );
    assert_eq!(
        metadata_before
            .selected_observation
            .as_ref()
            .unwrap()
            .provider_recording_id
            .as_deref(),
        Some(fixture::RECORDING)
    );

    f.publish_verified_album().await;
    let request_id = Uuid::new_v4();
    let request = || RefreshImportedPlaylistMetadata {
        request_id,
        installation_revision: f.installation.revision,
    };
    let refreshed = f
        .hito
        .collection
        .refresh_imported_playlist_metadata(&f.actor, &first_id, request())
        .await
        .expect("an approved playlist-only provider can repair an existing copy's missing albums");
    assert_eq!(refreshed.request_id, request_id);
    assert_eq!(refreshed.playlist_id, f.first.playlist_id);
    assert_eq!(refreshed.updated_recordings, 1);
    assert_eq!(refreshed.unchanged_recordings, 0);
    assert_eq!(refreshed.unavailable_recordings, 1);
    let refresh_receipt = serde_json::to_vec(&refreshed).unwrap();

    let albums = f
        .hito
        .catalog
        .releases(&f.actor, Default::default())
        .await
        .unwrap();
    assert_eq!(albums.total, 1);
    let album = &albums.items[0];
    assert_eq!(album.title, "Verified provider album");
    assert_eq!(album.track_count, 1);
    assert!(album.artist.is_none() && album.artist_id.is_none());
    let album_id = album.id.clone();
    let album = f
        .hito
        .catalog
        .release(&f.actor, &album_id, Default::default())
        .await
        .unwrap();
    assert_eq!(album.tracks.total, 1);
    let member = &album.tracks.items[0];
    assert_eq!(member.recording.id, recording);
    assert_eq!(member.disc_number, Some(1));
    assert_eq!(member.track_number, Some(4));
    assert_eq!(member.recording.title, "My private title");
    assert_eq!(member.recording.artist, "My private performer");
    assert!(member.recording.liked && member.recording.available);
    let member_id = member.entry_id.clone();

    let metadata_after = f
        .hito
        .catalog
        .recording_metadata(&f.actor, &recording, Default::default())
        .await
        .unwrap();
    assert_eq!(
        metadata_after.title.personal,
        metadata_before.title.personal
    );
    assert_eq!(
        metadata_after.artist.personal,
        metadata_before.artist.personal
    );
    assert_eq!(
        metadata_after.title.operation_id,
        metadata_before.title.operation_id
    );
    assert_eq!(
        metadata_after.artist.operation_id,
        metadata_before.artist.operation_id
    );
    assert_eq!(metadata_after.title.inherited, "Verified provider title");
    assert_eq!(metadata_after.artist.inherited, "Verified performer");
    assert_eq!(
        metadata_after.observations.items.len(),
        metadata_before.observations.items.len() + 1
    );
    for old in &metadata_before.observations.items {
        let retained = metadata_after
            .observations
            .items
            .iter()
            .find(|item| item.id == old.id)
            .unwrap();
        assert_eq!(
            serde_json::to_vec(retained).unwrap(),
            serde_json::to_vec(old).unwrap()
        );
    }
    let selected = metadata_after.selected_observation.as_ref().unwrap();
    assert_ne!(
        selected.id,
        metadata_before.selected_observation.as_ref().unwrap().id
    );
    assert_eq!(selected.release_id.as_deref(), Some(album_id.as_str()));
    assert_eq!(selected.provider, fixture::PROVIDER);
    assert_eq!(
        selected.provider_recording_id.as_deref(),
        Some(fixture::RECORDING)
    );
    assert_eq!(
        selected.provider_revision.as_deref(),
        Some("verified-album-one")
    );
    assert_eq!(selected.title, "Verified provider title");
    assert_eq!(selected.artist, "Verified performer");
    assert_eq!(selected.duration_ms, 91_000);
    assert_eq!(selected.album.as_deref(), Some("Verified provider album"));
    assert!(selected.album_artist.is_none() && selected.source_id.is_none());
    assert_eq!(selected.disc_number, Some(1));
    assert_eq!(selected.track_number, Some(4));
    let missing_after = f
        .hito
        .catalog
        .recording_metadata(&f.actor, &missing, Default::default())
        .await
        .unwrap();
    assert_eq!(
        serde_json::to_vec(&missing_after).unwrap(),
        serde_json::to_vec(&missing_before).unwrap()
    );

    let first_after = f
        .hito
        .collection
        .playlist(&f.actor, &first_id)
        .await
        .unwrap();
    let second_after = f
        .hito
        .collection
        .playlist(&f.actor, &second_id)
        .await
        .unwrap();
    assert_eq!(copy_contents(&first_after), copy_contents(&first_before));
    assert_eq!(copy_contents(&second_after), copy_contents(&second_before));
    assert_eq!(
        serde_json::to_vec(&f.hito.catalog.sources(&f.actor, &recording).await.unwrap()).unwrap(),
        sources_before
    );
    assert_eq!(
        serde_json::to_value(
            f.hito
                .catalog
                .source_choice(&f.actor, &recording)
                .await
                .unwrap()
        )
        .unwrap(),
        choice_before
    );
    assert_eq!(
        progress_state(&f.hito.collection.activity(&f.actor).await.unwrap()),
        progress_before
    );
    assert_eq!(
        f.hito
            .collection
            .saved_position(&f.actor, recording_id)
            .await
            .unwrap()
            .unwrap()
            .position_ms,
        4_200
    );
    let recordings = f
        .hito
        .catalog
        .recordings(&f.actor, Default::default())
        .await
        .unwrap();
    assert_eq!(recordings.total, 2);
    assert!(
        recordings
            .items
            .iter()
            .any(|item| item.id == missing && item.release_id.is_none())
    );
    assert!(matches!(
        f.hito.downloads.status(&f.actor, &recording).await,
        Err(Error::NotFound)
    ));
    assert!(matches!(
        f.hito.downloads.status(&f.actor, &missing).await,
        Err(Error::NotFound)
    ));

    let all_events = f.hito.collection.events(&f.actor, 0).await.unwrap();
    assert_eq!(
        serde_json::to_vec(&all_events.operations[..events_before.operations.len()]).unwrap(),
        old_event_bytes
    );
    let new_events = f
        .hito
        .collection
        .events(&f.actor, events_before.cursor)
        .await
        .unwrap();
    assert_eq!(new_events.operations.len(), 1);
    let event = serde_json::to_value(&new_events.operations[0]).unwrap();
    let publication = &event["publication"];
    assert_eq!(publication["kind"], "playlist_metadata_refreshed");
    assert!(Uuid::parse_str(publication["publication_id"].as_str().unwrap()).is_ok());
    assert_eq!(publication["playlist_id"], first_id);
    assert!(publication.get("entries").is_none() && publication.get("name").is_none());
    let origin = &publication["origin"];
    let imported_origin = first_before.origin.as_ref().unwrap();
    assert_eq!(origin["installation_id"], f.installation.id);
    assert_eq!(origin["installation_revision"], f.installation.revision);
    assert_eq!(origin["service_id"], imported_origin.service_id);
    assert_eq!(origin["service_revision"], imported_origin.service_revision);
    assert_eq!(origin["provider"], fixture::PROVIDER);
    assert_eq!(origin["provider_playlist_id"], fixture::PLAYLIST);
    assert_eq!(origin["snapshot"], "verified-album-snapshot");
    assert!(origin["observed_at"].as_i64().unwrap() > 0);
    assert!(origin.get("imported_at").is_none());
    let published_recordings = publication["recordings"].as_array().unwrap();
    assert_eq!(published_recordings.len(), 1);
    assert_eq!(published_recordings[0]["recording_id"], recording);
    assert_eq!(
        published_recordings[0]["library_id"],
        album.release.library_id
    );
    assert_eq!(published_recordings[0]["release_id"], album_id);
    assert_eq!(published_recordings[0]["observation_id"], selected.id);
    assert_eq!(
        published_recordings[0]["metadata"],
        serde_json::to_value(fixture::verified_metadata()).unwrap()
    );

    let detail_requests = f.origin.requests("v1/playlists/detail").await;
    assert_eq!(detail_requests.len(), detail_requests_before + 2);
    for (_, body) in &detail_requests[detail_requests_before..] {
        assert_eq!(
            *body,
            json!({"kind": "canonical", "playlist_id": fixture::PLAYLIST})
        );
    }
    assert_eq!(
        f.origin.requests("v1/playlists/items").await.len(),
        item_requests_before + 1
    );
    for path in [
        "v1/recordings/detail",
        "v1/recordings/metadata",
        "v1/search",
        "v1/sources",
        "v1/audio",
    ] {
        assert!(
            f.origin.requests(path).await.is_empty(),
            "unexpected origin request to {path}"
        );
    }
    assert_eq!(
        f.alternate.requests("v1/recordings/detail").await.len(),
        alternate_details_before
    );
    assert_eq!(
        f.alternate.requests("v1/sources").await.len(),
        alternate_sources_before
    );
    assert!(f.alternate.requests("v1/audio").await.is_empty());

    f.origin.offline().await;
    f.alternate.offline().await;
    let replayed = f
        .hito
        .collection
        .refresh_imported_playlist_metadata(&f.actor, &first_id, request())
        .await
        .unwrap();
    assert_eq!(serde_json::to_vec(&replayed).unwrap(), refresh_receipt);
    for (receipt, bytes) in [
        (&f.first, first_receipt_before),
        (&f.second, second_receipt_before),
    ] {
        let replayed = f
            .hito
            .collection
            .import_provider_playlist(
                &f.actor,
                &f.installation.id,
                fixture::import_request(&f.installation, receipt.request_id),
            )
            .await
            .unwrap();
        assert_eq!(serde_json::to_vec(&replayed).unwrap(), bytes);
    }
    assert_eq!(
        f.origin.requests("v1/playlists/detail").await.len(),
        detail_requests.len()
    );
    assert_eq!(
        f.origin.requests("v1/playlists/items").await.len(),
        item_requests_before + 1
    );
    let replayed_album = f
        .hito
        .catalog
        .release(&f.actor, &album_id, Default::default())
        .await
        .unwrap();
    assert_eq!(replayed_album.tracks.items[0].entry_id, member_id);
    assert_eq!(replayed_album.tracks.items[0].recording.id, recording);
    assert_eq!(
        serde_json::to_vec(
            &f.hito
                .catalog
                .recording_metadata(&f.actor, &recording, Default::default())
                .await
                .unwrap()
        )
        .unwrap(),
        serde_json::to_vec(&metadata_after).unwrap()
    );
    assert_eq!(
        serde_json::to_vec(&f.hito.collection.events(&f.actor, 0).await.unwrap()).unwrap(),
        serde_json::to_vec(&all_events).unwrap()
    );
    assert_eq!(
        copy_contents(
            &f.hito
                .collection
                .playlist(&f.actor, &first_id)
                .await
                .unwrap()
        ),
        copy_contents(&first_before)
    );
    assert_eq!(
        copy_contents(
            &f.hito
                .collection
                .playlist(&f.actor, &second_id)
                .await
                .unwrap()
        ),
        copy_contents(&second_before)
    );
}

fn copy_contents(detail: &PlaylistDetail) -> Value {
    json!({
        "playlist": detail.playlist,
        "origin": detail.origin,
        "follow_content_revision": detail.follow_content_revision,
        "entries": detail.entries.iter().map(|entry| json!({
            "entry_id": entry.entry_id,
            "recording_id": entry.recording_id,
            "position": entry.position,
            "origin": entry.origin,
        })).collect::<Vec<_>>(),
    })
}

fn progress_state(activity: &Activity) -> Value {
    json!(
        activity
            .history
            .iter()
            .map(|entry| json!({
                "recording_id": entry.id,
                "position_ms": entry.position_ms,
                "speed": entry.speed,
                "updated_at": entry.updated_at,
            }))
            .collect::<Vec<_>>()
    )
}
