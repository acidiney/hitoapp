use hito_core::test_support::playlist_service as fixture;
use hito_core::test_support::support;
use hito_core::{Browse, ImportProviderPlaylist, Search};
use serde_json::json;
use uuid::Uuid;

#[tokio::test]
async fn a_recording_plays_from_services_only_while_a_connected_service_can_resolve_it() {
    let server = fixture::ProviderServer::start().await;
    let (_directory, hito, actor, _library, _music) = support::library("plays-from-services").await;
    let installed = fixture::connect(&hito, &actor, &server, &["playlists", "sources"]).await;
    fixture::playlist(
        &server,
        "playlist/plays",
        "v1",
        vec![
            json!({"occurrence_id": "one", "item": {"kind": "recording", "recording": {
            "id": "one", "title": "One", "artist": "Band", "duration_ms": 100000,
            "release": {"id": "album-one", "title": "Album"}, "disc_number": 1, "track_number": 1,
            "revision": "v1"}}}),
        ],
    )
    .await;
    let copied = hito
        .collection
        .import_provider_playlist(
            &actor,
            &installed.id,
            ImportProviderPlaylist {
                request_id: Uuid::new_v4(),
                installation_revision: installed.revision,
                playlist_reference: "https://provider.invalid/playlist/plays".into(),
            },
        )
        .await
        .unwrap();

    let recordings = hito
        .catalog
        .recordings(&actor, Search::default())
        .await
        .unwrap();
    assert_eq!(recordings.items.len(), 1);
    assert!(recordings.items[0].plays_from_services);
    let playlist = hito
        .collection
        .playlist(&actor, &copied.playlist_id.to_string())
        .await
        .unwrap();
    assert!(playlist.entries[0].plays_from_services);
    let albums = hito
        .catalog
        .releases(&actor, Browse::default())
        .await
        .unwrap();
    let album = hito
        .catalog
        .release(&actor, &albums.items[0].id, Browse::default())
        .await
        .unwrap();
    assert!(album.tracks.items[0].recording.plays_from_services);

    hito.plugins
        .remove(&actor, &installed.id, installed.revision)
        .await
        .unwrap();
    let recordings = hito
        .catalog
        .recordings(&actor, Search::default())
        .await
        .unwrap();
    assert!(
        !recordings.items[0].plays_from_services,
        "No connected service remains"
    );
    let playlist = hito
        .collection
        .playlist(&actor, &copied.playlist_id.to_string())
        .await
        .unwrap();
    assert!(!playlist.entries[0].plays_from_services);
}

#[tokio::test]
async fn a_local_only_recording_does_not_play_from_services() {
    let (_directory, hito, actor, library, music) = support::library("plays-local").await;
    support::write_wav(&music.join("local.wav"), 3);
    support::scan(&hito, &actor, &library).await;
    let recordings = hito
        .catalog
        .recordings(&actor, Search::default())
        .await
        .unwrap();
    assert_eq!(recordings.items.len(), 1);
    assert!(recordings.items[0].available);
    assert!(!recordings.items[0].plays_from_services);
}
