use hito_core::test_support::playlist_service as fixture;
use hito_core::test_support::support;
use hito_core::{AlbumArtistInference, Browse, ImportProviderPlaylist};
use serde_json::{Value, json};
use uuid::Uuid;

fn track(id: &str, performer: &str, album: &str, album_artist: Option<&str>) -> Value {
    let mut release = json!({"id": album, "title": album});
    if let Some(artist) = album_artist {
        release["artist"] = json!(artist);
    }
    json!({"occurrence_id": id, "item": {"kind": "recording", "recording": {
        "id": id, "title": id, "artist": performer, "duration_ms": 100000,
        "release": release, "disc_number": 1, "track_number": 1, "revision": "v1"}}})
}

#[tokio::test]
async fn an_album_without_an_album_artist_shows_the_shared_track_artist_or_various_artists() {
    let server = fixture::ProviderServer::start().await;
    let (_directory, hito, actor, _library, _music) = support::library("album-inference").await;
    let installed = fixture::connect(&hito, &actor, &server, &["playlists"]).await;
    fixture::playlist(
        &server,
        "playlist/inference",
        "v1",
        vec![
            track("a1", "Same Band", "Shared", None),
            track("a2", "SAME BAND", "Shared", None),
            track("b1", "First Band", "Mixed", None),
            track("b2", "Second Band", "Mixed", None),
            track("c1", "Credited Band", "Credited", Some("Album Owner")),
        ],
    )
    .await;
    hito.collection
        .import_provider_playlist(
            &actor,
            &installed.id,
            ImportProviderPlaylist {
                request_id: Uuid::new_v4(),
                installation_revision: installed.revision,
                playlist_reference: "https://provider.invalid/playlist/inference".into(),
            },
        )
        .await
        .unwrap();
    let albums = hito
        .catalog
        .releases(&actor, Browse::default())
        .await
        .unwrap();
    assert_eq!(albums.total, 3);
    let by_title = |title: &str| albums.items.iter().find(|a| a.title == title).unwrap();

    let shared = by_title("Shared");
    assert!(shared.artist.is_none() && shared.artist_id.is_none());
    assert!(matches!(
        &shared.inferred_artist,
        Some(AlbumArtistInference::Named { name }) if name.eq_ignore_ascii_case("same band")
    ));
    let mixed = by_title("Mixed");
    assert!(mixed.artist.is_none());
    assert!(matches!(
        mixed.inferred_artist,
        Some(AlbumArtistInference::Various)
    ));
    let credited = by_title("Credited");
    assert_eq!(credited.artist.as_deref(), Some("Album Owner"));
    assert!(
        credited.inferred_artist.is_none(),
        "A stated credit is never inferred over"
    );

    let detail = hito
        .catalog
        .release(&actor, &mixed.id, Browse::default())
        .await
        .unwrap();
    assert!(matches!(
        detail.release.inferred_artist,
        Some(AlbumArtistInference::Various)
    ));
    assert!(
        detail.release.artist.is_none(),
        "Nothing is stored as an edit"
    );
}
