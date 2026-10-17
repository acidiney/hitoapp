#[path = "fixtures/playlist_service.rs"]
mod fixture;
mod support;

use hito_core::{
    Action, Actor, AdoptProviderRecording, CollectionPublication, Credentials, Error,
    ImportProviderPlaylist, ImportedPlaylistEntryKind, InstallPlugin, PlaylistDetail,
    PluginInstallation, Services, SyncEvent, SyncPage,
};
use serde_json::{Value, json};
use std::collections::HashSet;
use uuid::Uuid;

const ALICE_REFERENCE: &str = "https://provider.invalid/alice-selection";
const BOB_REFERENCE: &str = "https://provider.invalid/bob-selection";

#[tokio::test]
async fn same_request_ids_and_provider_items_remain_private_to_each_importing_account() {
    let server = fixture::ProviderServer::start().await;
    let (_dir, hito, alice, shared_library, _music) = support::library("playlist-isolation").await;
    let credentials = || Credentials {
        username: "bob".into(),
        password: "playlist bob password".into(),
    };
    hito.identity
        .create_user(&alice, credentials())
        .await
        .unwrap();
    let session = hito.identity.login(credentials()).await.unwrap();
    let bob = hito.identity.authenticate(&session.token).await.unwrap();
    let alice_install = fixture::connect(&hito, &alice, &server, &["playlists", "sources"]).await;
    let service_revision = hito
        .plugins
        .settings(&alice)
        .await
        .unwrap()
        .services
        .into_iter()
        .find(|service| service.id == alice_install.service_id)
        .unwrap()
        .revision;
    let bob_install = hito
        .plugins
        .install(
            &bob,
            InstallPlugin {
                service_id: alice_install.service_id.clone(),
                service_revision,
                bearer_token: None,
            },
        )
        .await
        .unwrap();
    assert_ne!(alice_install.id, bob_install.id);
    assert_eq!(alice_install.service_id, bob_install.service_id);
    for (actor, own_installation) in [(&alice, &alice_install), (&bob, &bob_install)] {
        let settings = hito.plugins.settings(actor).await.unwrap();
        assert_eq!(settings.installations.len(), 1);
        assert_eq!(settings.installations[0].id, own_installation.id);
    }
    let recording = json!({
        "id": "recording/shared", "title": "Provider recording", "artist": "Provider artist",
        "duration_ms": 90_000, "revision": "recording-one"
    });
    server
        .reply("v1/recordings/detail", recording.clone())
        .await;
    server
        .reply(
            "v1/sources",
            json!({"items": [
                {"id": "audio/shared", "label": "Original audio", "ready": true}
            ]}),
        )
        .await;
    let adopted = hito
        .catalog
        .adopt_provider(
            &alice,
            &alice_install.id,
            AdoptProviderRecording {
                installation_revision: alice_install.revision,
                provider_recording_id: "recording/shared".into(),
            },
        )
        .await
        .unwrap();
    let alice_recording = Uuid::parse_str(&adopted.id).unwrap();
    let alice_sources =
        serde_json::to_value(hito.catalog.sources(&alice, &adopted.id).await.unwrap()).unwrap();
    assert_eq!(alice_sources.as_array().unwrap().len(), 1);
    assert_eq!(alice_sources[0]["ready"], true);
    support::synchronize(
        &hito,
        &alice,
        vec![
            Action::RecordingTitleSet {
                recording_id: alice_recording,
                title: "Alice title".into(),
            },
            Action::RecordingArtistSet {
                recording_id: alice_recording,
                artist: "Alice artist".into(),
            },
            Action::LikeSet {
                recording_id: alice_recording,
                liked: true,
            },
            Action::ProgressSet {
                recording_id: alice_recording,
                position_ms: 4_200,
                speed: 1.25,
            },
        ],
    )
    .await;
    assert_eq!(
        hito.catalog
            .recordings(&bob, Default::default())
            .await
            .unwrap()
            .total,
        0
    );
    assert!(hito.collection.playlists(&bob).await.unwrap().is_empty());
    assert!(
        hito.collection
            .events(&bob, 0)
            .await
            .unwrap()
            .operations
            .is_empty()
    );
    fixture::playlist(&server, "playlist/shared", "playlist-one", vec![
        json!({"occurrence_id": "first", "item": {"kind": "recording", "recording": recording}}),
        json!({"occurrence_id": "second", "item": {"kind": "recording", "recording": recording}}),
        json!({"occurrence_id": "unknown", "item": {"kind": "unknown"}}),
    ]).await;
    let request_id = Uuid::new_v4();
    let alice_copy = hito
        .collection
        .import_provider_playlist(
            &alice,
            &alice_install.id,
            request(&alice_install, request_id, ALICE_REFERENCE),
        )
        .await
        .unwrap();
    let bob_copy = hito
        .collection
        .import_provider_playlist(
            &bob,
            &bob_install.id,
            request(&bob_install, request_id, BOB_REFERENCE),
        )
        .await
        .unwrap();
    assert_eq!(alice_copy.request_id, request_id);
    assert_eq!(bob_copy.request_id, request_id);
    assert_ne!(alice_copy.playlist_id, bob_copy.playlist_id);
    let alice_detail = hito
        .collection
        .playlist(&alice, &alice_copy.playlist_id.to_string())
        .await
        .unwrap();
    let bob_detail = hito
        .collection
        .playlist(&bob, &bob_copy.playlist_id.to_string())
        .await
        .unwrap();
    assert_eq!(
        alice_detail.origin.as_ref().unwrap().installation_id,
        alice_install.id
    );
    assert_eq!(
        bob_detail.origin.as_ref().unwrap().installation_id,
        bob_install.id
    );
    let bob_recording = bob_detail.entries[0].recording_id.as_deref().unwrap();
    assert_ne!(bob_recording, adopted.id);
    assert_rows(
        &alice_detail,
        &adopted.id,
        "Alice title",
        "Alice artist",
        true,
        true,
    );
    assert_rows(
        &bob_detail,
        bob_recording,
        "Provider recording",
        "Provider artist",
        false,
        false,
    );
    let entries = alice_detail
        .entries
        .iter()
        .chain(&bob_detail.entries)
        .map(|entry| Uuid::parse_str(&entry.entry_id).unwrap())
        .collect::<HashSet<_>>();
    assert_eq!(entries.len(), 6);
    let bob_recording_id = Uuid::parse_str(bob_recording).unwrap();
    let alice_catalog = hito
        .catalog
        .recordings(&alice, Default::default())
        .await
        .unwrap();
    let bob_catalog = hito
        .catalog
        .recordings(&bob, Default::default())
        .await
        .unwrap();
    assert_eq!(alice_catalog.total, 1);
    assert_eq!(bob_catalog.total, 1);
    assert_eq!(alice_catalog.items[0].id, adopted.id);
    assert_eq!(bob_catalog.items[0].id, bob_recording);
    let alice_library = &alice_catalog.items[0].library_id;
    let bob_library = &bob_catalog.items[0].library_id;
    assert_eq!(*alice_library, adopted.library_id);
    assert_ne!(alice_library, bob_library);
    assert_ne!(*alice_library, shared_library);
    assert_ne!(*bob_library, shared_library);
    for (actor, own_library, foreign_library) in [
        (&alice, alice_library, bob_library),
        (&bob, bob_library, alice_library),
    ] {
        let libraries = hito.catalog.libraries(actor).await.unwrap();
        assert!(libraries.iter().any(|library| library.id == *own_library));
        assert!(
            libraries
                .iter()
                .all(|library| library.id != *foreign_library)
        );
    }
    assert_eq!(
        serde_json::to_value(hito.catalog.sources(&alice, &adopted.id).await.unwrap()).unwrap(),
        alice_sources
    );
    assert!(
        hito.catalog
            .sources(&bob, bob_recording)
            .await
            .unwrap()
            .is_empty()
    );
    let alice_metadata = hito
        .catalog
        .recording_metadata(&alice, &adopted.id, Default::default())
        .await
        .unwrap();
    let bob_metadata = hito
        .catalog
        .recording_metadata(&bob, bob_recording, Default::default())
        .await
        .unwrap();
    assert_eq!(
        alice_metadata.title.personal.as_deref(),
        Some("Alice title")
    );
    assert_eq!(
        alice_metadata.artist.personal.as_deref(),
        Some("Alice artist")
    );
    assert!(bob_metadata.title.personal.is_none());
    assert!(bob_metadata.artist.personal.is_none());
    assert_eq!(
        hito.collection
            .saved_position(&alice, alice_recording)
            .await
            .unwrap()
            .unwrap()
            .position_ms,
        4_200
    );
    assert!(
        hito.collection
            .saved_position(&bob, bob_recording_id)
            .await
            .unwrap()
            .is_none()
    );
    let alice_history = hito.collection.activity(&alice).await.unwrap().history;
    assert_eq!(alice_history.len(), 1);
    assert_eq!(alice_history[0].id, adopted.id);
    assert_eq!(alice_history[0].position_ms, 4_200);
    assert_eq!(alice_history[0].speed, 1.25);
    assert!(
        hito.collection
            .activity(&bob)
            .await
            .unwrap()
            .history
            .is_empty()
    );
    support::synchronize(
        &hito,
        &alice,
        vec![Action::PlaylistRename {
            playlist_id: alice_copy.playlist_id,
            name: "Alice copy".into(),
        }],
    )
    .await;
    support::synchronize(
        &hito,
        &bob,
        vec![Action::PlaylistRename {
            playlist_id: bob_copy.playlist_id,
            name: "Bob copy".into(),
        }],
    )
    .await;
    for (actor, own_copy, name) in [
        (&alice, &alice_copy, "Alice copy"),
        (&bob, &bob_copy, "Bob copy"),
    ] {
        let playlists = hito.collection.playlists(actor).await.unwrap();
        assert_eq!(playlists.len(), 1);
        assert_eq!(playlists[0].id, own_copy.playlist_id.to_string());
        assert_eq!(playlists[0].name, name);
    }
    assert_feed(
        &hito.collection.events(&alice, 0).await.unwrap(),
        alice_copy.playlist_id,
        alice_recording,
        Uuid::parse_str(alice_library).unwrap(),
        5,
    );
    assert_feed(
        &hito.collection.events(&bob, 0).await.unwrap(),
        bob_copy.playlist_id,
        bob_recording_id,
        Uuid::parse_str(bob_library).unwrap(),
        1,
    );
    let before_denials = provider_counts(&server).await;
    for (actor, foreign_install, foreign_copy, foreign_recording, reference) in [
        (
            &alice,
            &bob_install,
            &bob_copy,
            bob_recording,
            ALICE_REFERENCE,
        ),
        (
            &bob,
            &alice_install,
            &alice_copy,
            adopted.id.as_str(),
            BOB_REFERENCE,
        ),
    ] {
        assert!(matches!(
            hito.collection
                .playlist(actor, &foreign_copy.playlist_id.to_string())
                .await,
            Err(Error::NotFound)
        ));
        assert!(matches!(
            hito.catalog.sources(actor, foreign_recording).await,
            Err(Error::NotFound)
        ));
        assert!(matches!(
            hito.catalog
                .recording_metadata(actor, foreign_recording, Default::default())
                .await,
            Err(Error::NotFound)
        ));
        assert!(matches!(
            hito.collection
                .saved_position(actor, Uuid::parse_str(foreign_recording).unwrap())
                .await,
            Err(Error::NotFound)
        ));
        assert!(matches!(
            hito.collection
                .import_provider_playlist(
                    actor,
                    &foreign_install.id,
                    request(foreign_install, Uuid::new_v4(), reference)
                )
                .await,
            Err(Error::NotFound)
        ));
        assert!(matches!(
            hito.collection
                .import_provider_playlist(
                    actor,
                    &foreign_install.id,
                    request(foreign_install, request_id, reference)
                )
                .await,
            Err(Error::Conflict(_))
        ));
    }
    assert_eq!(provider_counts(&server).await, before_denials);
    let alice_before = own_state(&hito, &alice, alice_copy.playlist_id, &adopted.id).await;
    let bob_before = own_state(&hito, &bob, bob_copy.playlist_id, bob_recording).await;
    server.offline().await;
    for (actor, installation, reference, original) in [
        (&alice, &alice_install, ALICE_REFERENCE, &alice_copy),
        (&bob, &bob_install, BOB_REFERENCE, &bob_copy),
    ] {
        let replay = hito
            .collection
            .import_provider_playlist(
                actor,
                &installation.id,
                request(installation, request_id, reference),
            )
            .await
            .unwrap();
        assert_eq!(
            serde_json::to_value(replay).unwrap(),
            serde_json::to_value(original).unwrap()
        );
    }
    assert_eq!(
        own_state(&hito, &alice, alice_copy.playlist_id, &adopted.id).await,
        alice_before
    );
    assert_eq!(
        own_state(&hito, &bob, bob_copy.playlist_id, bob_recording).await,
        bob_before
    );
    assert_eq!(provider_counts(&server).await, before_denials);
    assert_eq!(before_denials, [4, 2, 1, 1]);
}

fn request(
    installation: &PluginInstallation,
    request_id: Uuid,
    reference: &str,
) -> ImportProviderPlaylist {
    ImportProviderPlaylist {
        request_id,
        installation_revision: installation.revision,
        playlist_reference: reference.into(),
    }
}

fn assert_rows(
    detail: &PlaylistDetail,
    recording: &str,
    title: &str,
    artist: &str,
    liked: bool,
    available: bool,
) {
    assert_eq!(detail.playlist.kind, "local");
    assert_eq!(detail.entries.len(), 3);
    for (index, occurrence) in ["first", "second"].into_iter().enumerate() {
        let entry = &detail.entries[index];
        assert_eq!(entry.position, index as i64);
        assert_eq!(entry.recording_id.as_deref(), Some(recording));
        assert_eq!(entry.title, title);
        assert_eq!(entry.artist, artist);
        assert_eq!(entry.inherited_title, "Provider recording");
        assert_eq!(entry.inherited_artist, "Provider artist");
        assert_eq!(entry.liked, liked);
        assert_eq!(entry.available, available);
        assert_eq!(
            entry.origin.as_ref().unwrap().occurrence_id.as_deref(),
            Some(occurrence)
        );
        assert_eq!(
            entry
                .origin
                .as_ref()
                .unwrap()
                .provider_recording_id
                .as_deref(),
            Some("recording/shared")
        );
    }
    let unknown = &detail.entries[2];
    assert_eq!(unknown.position, 2);
    assert!(unknown.recording_id.is_none());
    assert!(!unknown.available);
    assert!(!unknown.liked);
    assert_eq!(unknown.title, "Unknown provider item");
    assert!(matches!(
        unknown.origin.as_ref().unwrap().kind,
        ImportedPlaylistEntryKind::Unknown
    ));
    assert!(
        unknown
            .origin
            .as_ref()
            .unwrap()
            .provider_recording_id
            .is_none()
    );
}

fn assert_feed(
    feed: &SyncPage,
    playlist: Uuid,
    recording: Uuid,
    library: Uuid,
    client_events: usize,
) {
    assert!(!feed.has_more);
    assert_eq!(feed.operations.len(), client_events + 1);
    let mut publications = 0;
    for event in &feed.operations {
        match event {
            SyncEvent::Operation { operation, .. } => match &operation.action {
                Action::RecordingTitleSet { recording_id, .. }
                | Action::RecordingArtistSet { recording_id, .. }
                | Action::LikeSet { recording_id, .. }
                | Action::ProgressSet { recording_id, .. } => assert_eq!(*recording_id, recording),
                Action::PlaylistRename { playlist_id, .. } => assert_eq!(*playlist_id, playlist),
                _ => panic!("only this account's edits belong in its feed"),
            },
            SyncEvent::Publication {
                publication: CollectionPublication::PlaylistImported { snapshot, .. },
                ..
            } => {
                publications += 1;
                assert_eq!(snapshot.playlist_id, playlist);
                assert_eq!(snapshot.name, "Fixture selection");
                assert_eq!(snapshot.entries.len(), 3);
                assert_eq!(snapshot.recordings.len(), 1);
                assert_eq!(snapshot.recordings[0].recording_id, recording);
                assert_eq!(snapshot.recordings[0].library_id, library);
                assert_eq!(snapshot.recordings[0].metadata.id, "recording/shared");
                assert_eq!(snapshot.recordings[0].metadata.title, "Provider recording");
            }
            SyncEvent::Publication {
                publication:
                    CollectionPublication::PlaylistFollowed { .. }
                    | CollectionPublication::PlaylistFollowRefreshed { .. },
                ..
            } => panic!("an import-only account feed must not contain follow publications"),
        }
    }
    assert_eq!(publications, 1);
}

async fn own_state(hito: &Services, actor: &Actor, playlist: Uuid, recording: &str) -> Value {
    json!({
        "catalog": hito.catalog.recordings(actor, Default::default()).await.unwrap(),
        "playlists": hito.collection.playlists(actor).await.unwrap(),
        "detail": hito.collection.playlist(actor, &playlist.to_string()).await.unwrap(),
        "sources": hito.catalog.sources(actor, recording).await.unwrap(),
        "metadata": hito.catalog.recording_metadata(actor, recording, Default::default()).await.unwrap(),
        "activity": hito.collection.activity(actor).await.unwrap(),
        "feed": hito.collection.events(actor, 0).await.unwrap()
    })
}

async fn provider_counts(server: &fixture::ProviderServer) -> [usize; 4] {
    [
        server.requests("v1/playlists/detail").await.len(),
        server.requests("v1/playlists/items").await.len(),
        server.requests("v1/recordings/detail").await.len(),
        server.requests("v1/sources").await.len(),
    ]
}
