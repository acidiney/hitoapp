#[path = "fixtures/playlist_service.rs"]
mod fixture;
mod support;

use hito_core::{
    Action, AdoptProviderRecording, Browse, Credentials, Error, ImportProviderPlaylist, Operation,
    SyncBatch, SyncEvent,
};
use serde_json::{Value, json};
use uuid::Uuid;

#[tokio::test]
async fn primary_credit_refresh_preserves_private_pins_and_reset_inherits_the_current_unknown_credit()
 {
    let server = fixture::ProviderServer::start().await;
    let (_directory, hito, actor, shared, _music) =
        support::library("release-credit-refresh").await;
    let installed = fixture::connect(&hito, &actor, &server, &["playlists", "sources"]).await;
    fixture::playlist(&server, "playlist/credit-refresh", "initial", vec![
        json!({"occurrence_id":"first","item":{"kind":"recording","recording":metadata("initial", "Initial performer", None)}}),
        json!({"occurrence_id":"again","item":{"kind":"recording","recording":metadata("initial", "Initial performer", None)}}),
    ]).await;
    server.reply("v1/sources", json!({"items":[]})).await;
    let request_id = Uuid::new_v4();
    let request = || ImportProviderPlaylist {
        request_id,
        installation_revision: installed.revision,
        playlist_reference: "https://provider.invalid/playlists/credit-refresh".into(),
    };
    let copied = hito
        .collection
        .import_provider_playlist(&actor, &installed.id, request())
        .await
        .unwrap();
    let playlist = hito
        .collection
        .playlist(&actor, &copied.playlist_id.to_string())
        .await
        .unwrap();
    assert_eq!(playlist.entries.len(), 2);
    let occurrence_ids = playlist
        .entries
        .iter()
        .map(|entry| entry.entry_id.clone())
        .collect::<Vec<_>>();
    assert_ne!(occurrence_ids[0], occurrence_ids[1]);
    let recording = playlist.entries[0].recording_id.clone().unwrap();
    assert_eq!(
        playlist.entries[1].recording_id.as_deref(),
        Some(recording.as_str())
    );
    let recording_id = Uuid::parse_str(&recording).unwrap();
    let albums = hito
        .catalog
        .releases(&actor, Browse::default())
        .await
        .unwrap();
    assert_eq!(albums.total, 1);
    let release = albums.items[0].id.clone();
    let library = albums.items[0].library_id.clone();
    let release_id = Uuid::parse_str(&release).unwrap();
    assert!(albums.items[0].artist.is_none() && albums.items[0].artist_id.is_none());
    let original_album = hito
        .catalog
        .release(&actor, &release, Browse::default())
        .await
        .unwrap();
    assert_eq!(original_album.tracks.total, 1);
    let appearance = original_album.tracks.items[0].entry_id.clone();
    assert!(Uuid::parse_str(&appearance).is_ok());
    assert_eq!(original_album.tracks.items[0].recording.id, recording);
    let initial_observation = hito
        .catalog
        .recording_metadata(&actor, &recording, Default::default())
        .await
        .unwrap()
        .selected_observation
        .unwrap();
    assert!(initial_observation.album_artist.is_none());
    let initial_observation = serde_json::to_value(initial_observation).unwrap();
    let initial_events = hito.collection.events(&actor, 0).await.unwrap();
    let original_publication = serde_json::to_value(&initial_events.operations[0]).unwrap();

    let pin = Uuid::new_v4();
    let liked = Uuid::new_v4();
    let progressed = Uuid::new_v4();
    let applied = hito
        .collection
        .apply(
            &actor,
            SyncBatch {
                device_id: "credit-refresh-client".into(),
                operations: vec![
                    Operation {
                        id: pin,
                        action: Action::ReleaseArtistSet {
                            release_id,
                            artist: "My album credit".into(),
                        },
                    },
                    Operation {
                        id: liked,
                        action: Action::LikeSet {
                            recording_id,
                            liked: true,
                        },
                    },
                    Operation {
                        id: progressed,
                        action: Action::ProgressSet {
                            recording_id,
                            position_ms: 43_210,
                            speed: 1.25,
                        },
                    },
                ],
            },
        )
        .await
        .unwrap();
    assert_eq!(applied.acknowledged, [pin, liked, progressed]);
    let pinned = hito
        .catalog
        .release(&actor, &release, Browse::default())
        .await
        .unwrap();
    let private_artist = pinned.release.artist_id.clone().unwrap();
    assert_eq!(pinned.release.artist.as_deref(), Some("My album credit"));

    for (revision, performer, credit, observation_count) in [
        (
            "verified",
            "Verified performer",
            Some("Verified album credit"),
            2,
        ),
        ("unknown-again", "Final performer", None, 3),
    ] {
        server
            .reply(
                "v1/recordings/detail",
                metadata(revision, performer, credit),
            )
            .await;
        let refreshed = hito
            .catalog
            .adopt_provider(
                &actor,
                &installed.id,
                AdoptProviderRecording {
                    installation_revision: installed.revision,
                    provider_recording_id: "recording/credit-refresh".into(),
                },
            )
            .await
            .unwrap();
        assert_eq!(refreshed.id, recording);
        assert_eq!(refreshed.release_id.as_deref(), Some(release.as_str()));
        assert_eq!(refreshed.artist, performer);
        assert!(refreshed.liked && !refreshed.available);
        assert!(
            hito.catalog
                .sources(&actor, &recording)
                .await
                .unwrap()
                .is_empty()
        );
        let detail = hito
            .catalog
            .release(&actor, &release, Browse::default())
            .await
            .unwrap();
        assert_eq!(detail.release.id, release);
        assert_eq!(detail.release.title, "Verified provider album");
        assert_eq!(detail.release.artist.as_deref(), Some("My album credit"));
        assert_eq!(
            detail.release.artist_id.as_deref(),
            Some(private_artist.as_str())
        );
        assert_eq!(detail.tracks.total, 1);
        assert_eq!(detail.tracks.items[0].entry_id, appearance);
        assert_eq!(detail.tracks.items[0].recording.id, recording);
        assert_eq!(detail.tracks.items[0].recording.artist, performer);
        assert_eq!(detail.tracks.items[0].track_number, Some(2));
        let metadata = hito
            .catalog
            .release_metadata(&actor, &release, Default::default())
            .await
            .unwrap();
        assert_eq!(metadata.artist.value.as_deref(), Some("My album credit"));
        assert_eq!(metadata.artist.personal.as_deref(), Some("My album credit"));
        assert_eq!(metadata.artist.inherited.as_deref(), credit);
        assert_eq!(
            metadata.artist.operation_id.as_deref(),
            Some(pin.to_string().as_str())
        );
        assert_eq!(metadata.observations.items.len(), observation_count);
        let original = metadata
            .observations
            .items
            .iter()
            .find(|observation| observation.provider_revision.as_deref() == Some("initial"))
            .unwrap();
        assert_eq!(serde_json::to_value(original).unwrap(), initial_observation);
        let verified = metadata
            .observations
            .items
            .iter()
            .find(|observation| observation.provider_revision.as_deref() == Some("verified"))
            .unwrap();
        assert_eq!(
            verified.album_artist.as_deref(),
            Some("Verified album credit")
        );
        assert_eq!(verified.artist, "Verified performer");
        let selected = hito
            .catalog
            .recording_metadata(&actor, &recording, Default::default())
            .await
            .unwrap()
            .selected_observation
            .unwrap();
        assert_eq!(selected.provider_revision.as_deref(), Some(revision));
        assert_eq!(selected.album_artist.as_deref(), credit);
        assert_eq!(selected.artist, performer);
        let artists = hito
            .catalog
            .artists(&actor, Browse::default())
            .await
            .unwrap();
        assert_eq!(artists.total, 2);
        let current_performer = artists
            .items
            .iter()
            .find(|artist| artist.name == performer)
            .unwrap();
        let artist = hito
            .catalog
            .artist(&actor, &current_performer.id, Browse::default())
            .await
            .unwrap();
        assert_eq!(artist.recordings.total, 1);
        assert_eq!(artist.recordings.items[0].id, recording);
        assert_eq!(artist.releases.total, 1);
        assert_eq!(artist.releases.items[0].id, release);
        let retained = hito
            .collection
            .playlist(&actor, &copied.playlist_id.to_string())
            .await
            .unwrap();
        assert_eq!(
            retained
                .entries
                .iter()
                .map(|entry| entry.entry_id.clone())
                .collect::<Vec<_>>(),
            occurrence_ids
        );
        assert_eq!(
            retained
                .entries
                .iter()
                .map(|entry| (
                    entry.position,
                    entry.recording_id.as_deref(),
                    entry.artist.as_str(),
                    entry.liked
                ))
                .collect::<Vec<_>>(),
            [
                (0, Some(recording.as_str()), performer, true),
                (1, Some(recording.as_str()), performer, true)
            ]
        );
        assert_eq!(
            hito.collection
                .saved_position(&actor, recording_id)
                .await
                .unwrap()
                .unwrap()
                .position_ms,
            43_210
        );
        let activity = hito.collection.activity(&actor).await.unwrap();
        assert_eq!(activity.history[0].id, recording);
        assert_eq!(activity.history[0].position_ms, 43_210);
        assert_eq!(activity.history[0].speed, 1.25);
        let replay = hito
            .collection
            .import_provider_playlist(&actor, &installed.id, request())
            .await
            .unwrap();
        assert_eq!(replay.playlist_id, copied.playlist_id);
    }

    let reset = Uuid::new_v4();
    hito.collection
        .apply(
            &actor,
            SyncBatch {
                device_id: "credit-refresh-client".into(),
                operations: vec![Operation {
                    id: reset,
                    action: Action::ReleaseArtistInherit { release_id },
                }],
            },
        )
        .await
        .unwrap();
    let metadata = hito
        .catalog
        .release_metadata(&actor, &release, Default::default())
        .await
        .unwrap();
    assert!(
        metadata.artist.value.is_none()
            && metadata.artist.inherited.is_none()
            && metadata.artist.personal.is_none()
    );
    assert_eq!(
        metadata.artist.operation_id.as_deref(),
        Some(reset.to_string().as_str())
    );
    assert_eq!(metadata.observations.items.len(), 3);
    let detail = hito
        .catalog
        .release(&actor, &release, Browse::default())
        .await
        .unwrap();
    assert!(detail.release.artist.is_none() && detail.release.artist_id.is_none());
    assert_eq!(detail.tracks.items[0].entry_id, appearance);
    let retained = hito
        .collection
        .playlist(&actor, &copied.playlist_id.to_string())
        .await
        .unwrap();
    assert_eq!(
        retained
            .entries
            .iter()
            .map(|entry| entry.entry_id.clone())
            .collect::<Vec<_>>(),
        occurrence_ids
    );
    assert_eq!(
        retained
            .entries
            .iter()
            .map(|entry| (entry.position, entry.recording_id.as_deref(), entry.liked))
            .collect::<Vec<_>>(),
        [
            (0, Some(recording.as_str()), true),
            (1, Some(recording.as_str()), true)
        ]
    );
    assert_eq!(
        hito.collection
            .saved_position(&actor, recording_id)
            .await
            .unwrap()
            .unwrap()
            .position_ms,
        43_210
    );
    let artists = hito
        .catalog
        .artists(&actor, Browse::default())
        .await
        .unwrap();
    assert_eq!(artists.total, 1);
    assert_eq!(artists.items[0].name, "Final performer");
    assert!(matches!(
        hito.catalog
            .artist(&actor, &private_artist, Browse::default())
            .await,
        Err(Error::NotFound)
    ));
    let events = hito.collection.events(&actor, 0).await.unwrap();
    let retained_publication = events
        .operations
        .iter()
        .find(|event| matches!(event, SyncEvent::Publication { .. }))
        .unwrap();
    assert_eq!(
        serde_json::to_value(retained_publication).unwrap(),
        original_publication
    );
    assert_eq!(
        hito.catalog
            .releases(
                &actor,
                Browse {
                    library: Some(shared),
                    ..Default::default()
                }
            )
            .await
            .unwrap()
            .total,
        0
    );

    hito.identity
        .create_user(
            &actor,
            Credentials {
                username: "bob".into(),
                password: "credit refresh privacy password".into(),
            },
        )
        .await
        .unwrap();
    let session = hito
        .identity
        .login(Credentials {
            username: "bob".into(),
            password: "credit refresh privacy password".into(),
        })
        .await
        .unwrap();
    let bob = hito.identity.authenticate(&session.token).await.unwrap();
    assert_eq!(
        hito.catalog
            .releases(
                &bob,
                Browse {
                    library: Some(library),
                    ..Default::default()
                }
            )
            .await
            .unwrap()
            .total,
        0
    );
    assert!(matches!(
        hito.catalog
            .release(&bob, &release, Browse::default())
            .await,
        Err(Error::NotFound)
    ));
    assert!(matches!(
        hito.catalog
            .release_metadata(&bob, &release, Default::default())
            .await,
        Err(Error::NotFound)
    ));
    assert!(matches!(
        hito.collection
            .playlist(&bob, &copied.playlist_id.to_string())
            .await,
        Err(Error::NotFound)
    ));
    assert!(matches!(
        hito.catalog
            .artist(&bob, &artists.items[0].id, Browse::default())
            .await,
        Err(Error::NotFound)
    ));
    hito.plugins
        .remove(&actor, &installed.id, installed.revision)
        .await
        .unwrap();
    server.offline().await;
    let retained = hito
        .catalog
        .release(&actor, &release, Browse::default())
        .await
        .unwrap();
    assert_eq!(retained.tracks.total, 1);
    assert_eq!(retained.tracks.items[0].entry_id, appearance);
    assert!(retained.release.artist.is_none());
    assert!(server.requests("v1/audio").await.is_empty());
    assert_eq!(server.requests("v1/recordings/detail").await.len(), 2);
}

fn metadata(revision: &str, performer: &str, album_artist: Option<&str>) -> Value {
    let mut release = json!({"id":"album/credit-refresh","title":"Verified provider album"});
    if let Some(artist) = album_artist {
        release["artist"] = json!(artist);
    }
    json!({
        "id":"recording/credit-refresh","title":"Verified recording","artist":performer,
        "duration_ms":123000,"release":release,"disc_number":1,"track_number":2,"revision":revision
    })
}
