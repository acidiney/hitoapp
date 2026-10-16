#[path = "fixtures/playlist_service.rs"]
mod fixture;
mod support;

use hito_core::{Actor, Error, ImportProviderPlaylist, Services};
use hito_plugin_sdk::{PlaylistPage, ProviderPlaylist};
use serde_json::{Value, json};
use std::time::Duration;
use uuid::Uuid;

const PLAYLIST: &str = "playlist/consistent";
const SNAPSHOT: &str = "snapshot-one";
const REFERENCE: &str = "https://provider.invalid/consistent-selection";

#[derive(Clone, Copy, Debug)]
enum Inconsistency {
    CountShortfall,
    CursorAfterDeclaredEnd,
    WrongPlaylist,
    WrongSnapshot,
    CursorLoop,
    RepeatedOccurrence,
    ConflictingRecording,
    ChangedFinalDetail,
}

#[tokio::test]
async fn inconsistent_complete_snapshots_are_rejected_without_any_collection_or_catalog_changes() {
    for inconsistency in [
        Inconsistency::CountShortfall,
        Inconsistency::CursorAfterDeclaredEnd,
        Inconsistency::WrongPlaylist,
        Inconsistency::WrongSnapshot,
        Inconsistency::CursorLoop,
        Inconsistency::RepeatedOccurrence,
        Inconsistency::ConflictingRecording,
        Inconsistency::ChangedFinalDetail,
    ] {
        let server = fixture::ProviderServer::start().await;
        let (_dir, hito, actor, _shared, _music) = support::library("playlist-consistency").await;
        let installed = fixture::connect(&hito, &actor, &server, &["playlists"]).await;
        let (detail, pages, message) = snapshot(inconsistency);
        let typed_detail: ProviderPlaylist = serde_json::from_value(detail.clone()).unwrap();
        typed_detail.validate().unwrap();
        let mut source_bytes = serde_json::to_vec(&detail).unwrap().len() * 2;
        for (_, page) in &pages {
            let typed: PlaylistPage = serde_json::from_value(page.clone()).unwrap();
            typed.validate(100).unwrap();
            let encoded = serde_json::to_vec(page).unwrap().len();
            assert!(encoded < 1024 * 1024);
            source_bytes += encoded;
        }
        assert!(source_bytes < 16 * 1024 * 1024);
        let expected_cursors = pages
            .iter()
            .map(|(cursor, _)| cursor.clone())
            .collect::<Vec<_>>();
        server.reply("v1/playlists/detail", detail.clone()).await;
        if pages.len() == 1 {
            let page = pages.into_iter().next().unwrap().1;
            if matches!(inconsistency, Inconsistency::ChangedFinalDetail) {
                fixture::playlist(
                    &server,
                    PLAYLIST,
                    SNAPSHOT,
                    page["items"].as_array().unwrap().clone(),
                )
                .await;
            } else {
                server.reply("v1/playlists/items", page).await;
            }
        } else {
            server.playlist_pages(pages).await;
        }
        let gate = if matches!(inconsistency, Inconsistency::ChangedFinalDetail) {
            Some(server.hold("v1/playlists/items").await)
        } else {
            None
        };
        let before = public_state(&hito, &actor).await;
        assert_eq!(before["catalog"]["total"], 0);
        assert!(before["playlists"].as_array().unwrap().is_empty());
        assert!(before["feed"]["operations"].as_array().unwrap().is_empty());
        let pending = {
            let (collection, actor, installation_id) =
                (hito.collection.clone(), actor.clone(), installed.id.clone());
            let request = ImportProviderPlaylist {
                request_id: Uuid::new_v4(),
                installation_revision: installed.revision,
                playlist_reference: REFERENCE.into(),
            };
            tokio::spawn(async move {
                collection
                    .import_provider_playlist(&actor, &installation_id, request)
                    .await
            })
        };
        if let Some(gate) = gate {
            tokio::time::timeout(Duration::from_secs(3), gate.entered())
                .await
                .expect("the original item page must be held before final detail changes");
            let mut changed = detail;
            changed["snapshot"] = json!("snapshot-two");
            serde_json::from_value::<ProviderPlaylist>(changed.clone())
                .unwrap()
                .validate()
                .unwrap();
            server.reply("v1/playlists/detail", changed).await;
            assert_eq!(public_state(&hito, &actor).await, before);
            gate.release();
        }
        let result = tokio::time::timeout(Duration::from_secs(3), pending)
            .await
            .expect("bounded inconsistent snapshots must finish promptly")
            .unwrap();
        match result {
            Err(Error::Upstream(actual)) => assert_eq!(actual, message, "{inconsistency:?}"),
            _ => panic!("{inconsistency:?} must fail as an inconsistent upstream snapshot"),
        }
        assert_eq!(
            public_state(&hito, &actor).await,
            before,
            "{inconsistency:?}"
        );
        let detail_requests = server.requests("v1/playlists/detail").await;
        assert_eq!(
            detail_requests[0].1,
            json!({"kind": "reference", "reference": REFERENCE})
        );
        if matches!(inconsistency, Inconsistency::ChangedFinalDetail) {
            assert_eq!(detail_requests.len(), 2);
            assert_eq!(
                detail_requests[1].1,
                json!({"kind": "canonical", "playlist_id": PLAYLIST})
            );
        } else {
            assert_eq!(detail_requests.len(), 1, "{inconsistency:?}");
        }
        let item_requests = server.requests("v1/playlists/items").await;
        assert_eq!(
            item_requests.len(),
            expected_cursors.len(),
            "{inconsistency:?}"
        );
        for ((_, request), cursor) in item_requests.iter().zip(expected_cursors) {
            assert_eq!(
                *request,
                json!({
                    "playlist_id": PLAYLIST, "snapshot": SNAPSHOT, "cursor": cursor, "limit": 100
                })
            );
        }
    }
}

fn snapshot(inconsistency: Inconsistency) -> (Value, Vec<(Option<String>, Value)>, &'static str) {
    let total = match inconsistency {
        Inconsistency::CountShortfall
        | Inconsistency::RepeatedOccurrence
        | Inconsistency::ConflictingRecording => 2,
        Inconsistency::CursorLoop => 3,
        _ => 1,
    };
    let detail = json!({
        "id": PLAYLIST, "name": "Fixture selection", "snapshot": SNAPSHOT,
        "total_count": total, "attribution": {"label": "Playlist fixture"}
    });
    let first = entry("recording/first", "First recording", "occurrence-first");
    let second = entry("recording/second", "Second recording", "occurrence-second");
    let incomplete = "The provider returned an incomplete playlist";
    let changed = "The provider playlist changed during import";
    let (pages, message) = match inconsistency {
        Inconsistency::CountShortfall => (
            vec![(None, page(PLAYLIST, SNAPSHOT, first, None))],
            incomplete,
        ),
        Inconsistency::CursorAfterDeclaredEnd => (
            vec![(None, page(PLAYLIST, SNAPSHOT, first, Some("after-end")))],
            incomplete,
        ),
        Inconsistency::WrongPlaylist => (
            vec![(None, page("playlist/other", SNAPSHOT, first, None))],
            changed,
        ),
        Inconsistency::WrongSnapshot => (
            vec![(None, page(PLAYLIST, "snapshot-other", first, None))],
            changed,
        ),
        Inconsistency::CursorLoop => (
            vec![
                (None, page(PLAYLIST, SNAPSHOT, first, Some("loop"))),
                (
                    Some("loop".into()),
                    page(PLAYLIST, SNAPSHOT, second, Some("loop")),
                ),
            ],
            incomplete,
        ),
        Inconsistency::RepeatedOccurrence => (
            vec![
                (None, page(PLAYLIST, SNAPSHOT, first, Some("second"))),
                (
                    Some("second".into()),
                    page(
                        PLAYLIST,
                        SNAPSHOT,
                        entry("recording/second", "Second recording", "occurrence-first"),
                        None,
                    ),
                ),
            ],
            "The provider repeated a playlist occurrence ID",
        ),
        Inconsistency::ConflictingRecording => (
            vec![
                (None, page(PLAYLIST, SNAPSHOT, first, Some("second"))),
                (
                    Some("second".into()),
                    page(
                        PLAYLIST,
                        SNAPSHOT,
                        entry("recording/first", "Changed recording", "occurrence-second"),
                        None,
                    ),
                ),
            ],
            "The provider returned inconsistent recording metadata",
        ),
        Inconsistency::ChangedFinalDetail => {
            (vec![(None, page(PLAYLIST, SNAPSHOT, first, None))], changed)
        }
    };
    (detail, pages, message)
}

fn entry(id: &str, title: &str, occurrence: &str) -> Value {
    json!({"occurrence_id": occurrence, "item": {"kind": "recording", "recording": {
        "id": id, "title": title, "artist": "Fixture artist"
    }}})
}

fn page(id: &str, snapshot: &str, item: Value, next: Option<&str>) -> Value {
    json!({"playlist_id": id, "snapshot": snapshot, "items": [item], "next_cursor": next})
}

async fn public_state(hito: &Services, actor: &Actor) -> Value {
    json!({
        "catalog": hito.catalog.recordings(actor, Default::default()).await.unwrap(),
        "playlists": hito.collection.playlists(actor).await.unwrap(),
        "feed": hito.collection.events(actor, 0).await.unwrap()
    })
}
