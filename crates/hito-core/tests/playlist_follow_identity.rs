#[path = "fixtures/playlist_service.rs"]
mod fixture;
mod support;

use hito_core::{
    Action, FollowProviderPlaylist, ImportProviderPlaylist, ImportedPlaylistEntryKind,
    PlaylistFollowInterval, RefreshFollowedPlaylist,
};
use serde_json::{Value, json};
use uuid::Uuid;

fn recording(occurrence: &str) -> Value {
    json!({"occurrence_id": occurrence, "item": {"kind": "recording", "recording": {
        "id": "recording/stable", "title": "Stable recording", "artist": "Fixture artist"
    }}})
}

async fn refresh(hito: &hito_core::Services, actor: &hito_core::Actor, playlist: &str) {
    hito.collection
        .enqueue_follow_refresh(
            actor,
            playlist,
            RefreshFollowedPlaylist {
                request_id: Uuid::new_v4(),
            },
        )
        .await
        .unwrap();
    assert!(
        hito.collection
            .run_next_follow_refresh_until(std::future::pending())
            .await
            .unwrap()
    );
}

#[tokio::test]
async fn a_stable_provider_occurrence_keeps_its_entry_id_when_the_recording_becomes_unavailable_and_returns()
 {
    let server = fixture::ProviderServer::start().await;
    let (_directory, hito, actor, _shared, _music) = support::library("follow-identity").await;
    let installation = fixture::connect(&hito, &actor, &server, &["playlists"]).await;
    fixture::playlist(
        &server,
        "playlist/stable",
        "initial",
        vec![recording("first"), recording("second")],
    )
    .await;
    let copied = hito
        .collection
        .import_provider_playlist(
            &actor,
            &installation.id,
            ImportProviderPlaylist {
                request_id: Uuid::new_v4(),
                installation_revision: installation.revision,
                playlist_reference: "https://provider.invalid/stable-selection".into(),
            },
        )
        .await
        .unwrap();
    let copied_detail = hito
        .collection
        .playlist(&actor, &copied.playlist_id.to_string())
        .await
        .unwrap();
    let recording_id =
        Uuid::parse_str(copied_detail.entries[0].recording_id.as_ref().unwrap()).unwrap();
    support::synchronize(
        &hito,
        &actor,
        vec![
            Action::PlaylistRename {
                playlist_id: copied.playlist_id,
                name: "My independent selection".into(),
            },
            Action::EntryRemove {
                playlist_id: copied.playlist_id,
                entry_id: Uuid::parse_str(&copied_detail.entries[1].entry_id).unwrap(),
            },
            Action::RecordingTitleSet {
                recording_id,
                title: "My personal title".into(),
            },
        ],
    )
    .await;
    let copy_before = serde_json::to_value(
        hito.collection
            .playlist(&actor, &copied.playlist_id.to_string())
            .await
            .unwrap(),
    )
    .unwrap();
    let followed = hito
        .collection
        .follow_provider_playlist(
            &actor,
            &installation.id,
            FollowProviderPlaylist {
                request_id: Uuid::new_v4(),
                installation_revision: installation.revision,
                playlist_reference: "https://provider.invalid/stable-selection".into(),
                interval: PlaylistFollowInterval::Hourly,
            },
        )
        .await
        .unwrap();
    let playlist = followed.playlist_id.to_string();
    let initial = hito.collection.playlist(&actor, &playlist).await.unwrap();
    assert_eq!(initial.entries.len(), 2);
    assert_eq!(
        initial.entries[0].recording_id.as_deref(),
        Some(recording_id.to_string().as_str())
    );
    assert_eq!(
        initial.entries[0].recording_id,
        initial.entries[1].recording_id
    );
    let first_id = initial.entries[0].entry_id.clone();
    let second_id = initial.entries[1].entry_id.clone();
    assert_ne!(first_id, second_id);

    fixture::playlist(&server, "playlist/stable", "unavailable", vec![
        recording("second"),
        json!({"occurrence_id": "first", "item": {"kind": "unavailable", "provider_recording_id": "recording/stable", "title": "Temporarily unavailable", "artist": "Fixture artist"}}),
    ]).await;
    refresh(&hito, &actor, &playlist).await;
    let unavailable = hito.collection.playlist(&actor, &playlist).await.unwrap();
    assert_eq!(unavailable.entries.len(), 2);
    assert_eq!(unavailable.entries[0].entry_id, second_id);
    assert_eq!(unavailable.entries[0].position, 0);
    assert_eq!(unavailable.entries[1].position, 1);
    assert_eq!(
        unavailable.entries[1].entry_id, first_id,
        "A stable occurrence with the same provider recording witness must keep its entry UUID across unavailability"
    );
    assert!(unavailable.entries[1].recording_id.is_none());
    assert!(!unavailable.entries[1].available);
    assert_eq!(unavailable.entries[1].title, "Temporarily unavailable");
    let origin = unavailable.entries[1].origin.as_ref().unwrap();
    assert!(matches!(
        origin.kind,
        ImportedPlaylistEntryKind::Unavailable
    ));
    assert_eq!(origin.occurrence_id.as_deref(), Some("first"));
    assert_eq!(
        origin.provider_recording_id.as_deref(),
        Some("recording/stable")
    );
    assert_eq!(
        serde_json::to_value(
            hito.collection
                .playlist(&actor, &copied.playlist_id.to_string())
                .await
                .unwrap()
        )
        .unwrap(),
        copy_before
    );

    fixture::playlist(
        &server,
        "playlist/stable",
        "returned",
        vec![recording("first"), recording("second")],
    )
    .await;
    refresh(&hito, &actor, &playlist).await;
    let returned = hito.collection.playlist(&actor, &playlist).await.unwrap();
    assert_eq!(returned.entries.len(), 2);
    assert_eq!(returned.entries[0].entry_id, first_id);
    assert_eq!(returned.entries[1].entry_id, second_id);
    assert_eq!(
        returned
            .entries
            .iter()
            .map(|entry| entry.position)
            .collect::<Vec<_>>(),
        vec![0, 1]
    );
    assert!(
        returned
            .entries
            .iter()
            .all(
                |entry| entry.recording_id.as_deref() == Some(recording_id.to_string().as_str())
                    && entry.title == "My personal title"
            )
    );
    assert!(returned.entries.iter().all(|entry| matches!(
        entry.origin.as_ref().unwrap().kind,
        ImportedPlaylistEntryKind::Recording
    )));
    assert_eq!(
        serde_json::to_value(
            hito.collection
                .playlist(&actor, &copied.playlist_id.to_string())
                .await
                .unwrap()
        )
        .unwrap(),
        copy_before
    );
    assert_eq!(
        hito.catalog
            .recordings(&actor, Default::default())
            .await
            .unwrap()
            .total,
        1
    );
}
