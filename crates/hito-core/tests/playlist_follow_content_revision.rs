#[path = "fixtures/playlist_service.rs"]
mod fixture;
mod support;

use hito_core::{
    CollectionPublication, FollowProviderPlaylist, PlaylistFollowInterval, RefreshFollowedPlaylist,
    SyncEvent,
};
use serde_json::{Value, json};
use uuid::Uuid;

fn recording(occurrence: &str, id: &str, title: &str, revision: &str) -> Value {
    json!({
        "occurrence_id": occurrence,
        "item": {"kind": "recording", "recording": {
            "id": id, "title": title, "artist": "Fixture artist", "revision": revision
        }}
    })
}

#[tokio::test]
async fn complete_content_publication_advances_a_hito_revision_when_the_provider_reuses_its_token()
{
    let server = fixture::ProviderServer::start().await;
    let (_directory, hito, actor, _shared, _music) =
        support::library("follow-content-revision").await;
    let installation = fixture::connect(&hito, &actor, &server, &["playlists"]).await;
    let first_a = recording("first-a", "recording/A", "Provider A", "one");
    let second_a = recording("second-a", "recording/A", "Provider A", "one");
    fixture::playlist(
        &server,
        "playlist/content-revision",
        "reused-provider-token",
        vec![
            first_a.clone(),
            recording("b", "recording/B", "Provider B", "one"),
            second_a.clone(),
        ],
    )
    .await;
    let followed = hito
        .collection
        .follow_provider_playlist(
            &actor,
            &installation.id,
            FollowProviderPlaylist {
                request_id: Uuid::new_v4(),
                installation_revision: installation.revision,
                playlist_reference: "https://provider.invalid/content-revision-selection".into(),
                interval: PlaylistFollowInterval::Hourly,
            },
        )
        .await
        .unwrap();
    let playlist = followed.playlist_id.to_string();
    let initial =
        serde_json::to_value(hito.collection.playlist(&actor, &playlist).await.unwrap()).unwrap();
    let initial_status = serde_json::to_value(
        hito.collection
            .playlist_follow(&actor, &playlist)
            .await
            .unwrap(),
    )
    .unwrap();
    assert_eq!(initial_status["content_revision"], json!(1));
    assert_eq!(initial["follow_content_revision"], json!(1));
    assert_eq!(initial_status["snapshot"], "reused-provider-token");
    assert_eq!(initial["origin"]["snapshot"], "reused-provider-token");
    let initial_entries = initial["entries"].as_array().unwrap();
    assert_eq!(initial_entries.len(), 3);
    assert_ne!(
        initial_entries[0]["entry_id"],
        initial_entries[2]["entry_id"]
    );
    assert_eq!(
        initial_entries[0]["recording_id"],
        initial_entries[2]["recording_id"]
    );
    let cursor = hito.collection.events(&actor, 0).await.unwrap().cursor;

    fixture::playlist(
        &server,
        "playlist/content-revision",
        "reused-provider-token",
        vec![
            recording("b", "recording/B", "Revised provider B", "two"),
            second_a,
            recording("c", "recording/C", "Provider C", "one"),
            first_a,
        ],
    )
    .await;
    hito.collection
        .enqueue_follow_refresh(
            &actor,
            &playlist,
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

    let current =
        serde_json::to_value(hito.collection.playlist(&actor, &playlist).await.unwrap()).unwrap();
    let status = serde_json::to_value(
        hito.collection
            .playlist_follow(&actor, &playlist)
            .await
            .unwrap(),
    )
    .unwrap();
    assert_eq!(status["content_revision"], json!(2));
    assert_eq!(current["follow_content_revision"], json!(2));
    assert_eq!(status["snapshot"], initial_status["snapshot"]);
    assert_eq!(current["origin"]["snapshot"], initial["origin"]["snapshot"]);
    let entries = current["entries"].as_array().unwrap();
    assert_eq!(entries.len(), 4);
    assert_eq!(
        entries
            .iter()
            .map(|entry| entry["origin"]["occurrence_id"].as_str().unwrap())
            .collect::<Vec<_>>(),
        vec!["b", "second-a", "c", "first-a"]
    );
    assert_eq!(
        entries
            .iter()
            .map(|entry| entry["title"].as_str().unwrap())
            .collect::<Vec<_>>(),
        vec![
            "Revised provider B",
            "Provider A",
            "Provider C",
            "Provider A"
        ]
    );
    for (current_index, initial_index) in [(0, 1), (1, 2), (3, 0)] {
        assert_eq!(
            entries[current_index]["entry_id"],
            initial_entries[initial_index]["entry_id"]
        );
        assert_eq!(
            entries[current_index]["recording_id"],
            initial_entries[initial_index]["recording_id"]
        );
    }
    assert!(
        initial_entries
            .iter()
            .all(|entry| entry["entry_id"] != entries[2]["entry_id"])
    );
    let events = hito.collection.events(&actor, cursor).await.unwrap();
    assert_eq!(events.operations.len(), 1);
    let SyncEvent::Publication {
        publication: CollectionPublication::PlaylistFollowRefreshed { snapshot, .. },
        ..
    } = &events.operations[0]
    else {
        panic!("Changed complete content must publish the followed playlist");
    };
    assert_eq!(snapshot.origin.snapshot, "reused-provider-token");
    assert_eq!(
        snapshot
            .entries
            .iter()
            .map(|entry| entry.entry_id.to_string())
            .collect::<Vec<_>>(),
        entries
            .iter()
            .map(|entry| entry["entry_id"].as_str().unwrap().to_owned())
            .collect::<Vec<_>>()
    );
}
