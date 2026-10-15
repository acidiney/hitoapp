mod support;

use hito_core::{Action, Browse, Search};
use support::{catalog, synchronize};
use uuid::Uuid;

#[tokio::test]
async fn personal_title_is_used_by_search_albums_playlists_and_history() {
    let (_dir, hito, actor, _library) = catalog().await;
    let track = hito
        .catalog
        .recordings(&actor, Search::default())
        .await
        .unwrap()
        .items
        .remove(0);
    let recording = Uuid::parse_str(&track.id).unwrap();
    let playlist = Uuid::new_v4();
    synchronize(
        &hito,
        &actor,
        vec![
            Action::PlaylistCreate {
                playlist_id: playlist,
                name: "Personal collection".into(),
            },
            Action::EntryInsert {
                playlist_id: playlist,
                entry_id: Uuid::new_v4(),
                recording_id: recording,
                position: 0,
            },
            Action::ProgressSet {
                recording_id: recording,
                position_ms: 100,
                speed: 1.0,
            },
            Action::RecordingTitleSet {
                recording_id: recording,
                title: "Étoile personnelle".into(),
            },
        ],
    )
    .await;
    let page = hito
        .catalog
        .recordings(
            &actor,
            Search {
                q: Some("etoile Unknown".into()),
                ..Search::default()
            },
        )
        .await
        .unwrap();
    assert_eq!(page.total, 1);
    assert_eq!(page.items[0].id, track.id);
    assert_eq!(page.items[0].title, "Étoile personnelle");
    let all = hito
        .catalog
        .recordings(&actor, Search::default())
        .await
        .unwrap();
    assert_eq!(all.items.last().unwrap().id, track.id);
    let old = hito
        .catalog
        .recordings(
            &actor,
            Search {
                q: Some(track.title.clone()),
                ..Search::default()
            },
        )
        .await
        .unwrap();
    assert!(old.items.iter().all(|item| item.id != track.id));
    assert_eq!(
        hito.collection
            .playlist(&actor, &playlist.to_string())
            .await
            .unwrap()
            .entries[0]
            .title,
        "Étoile personnelle"
    );
    assert_eq!(
        hito.collection.activity(&actor).await.unwrap().history[0].title,
        "Étoile personnelle"
    );
    let album = hito
        .catalog
        .release(
            &actor,
            track.release_id.as_ref().unwrap(),
            Browse::default(),
        )
        .await
        .unwrap();
    assert_eq!(
        album
            .tracks
            .items
            .iter()
            .find(|item| item.recording.id == track.id)
            .unwrap()
            .recording
            .title,
        "Étoile personnelle"
    );
}
