use crate::audiobooks_fixture::Library;
use axum::http::StatusCode;
use hito_core::Action;
use serde_json::{Value, json};
use uuid::Uuid;

#[tokio::test]
async fn a_listener_can_browse_every_owned_book_past_the_legacy_cap() {
    let library = Library::prepare().await;
    let ids: Vec<Uuid> = (1..=121).map(Uuid::from_u128).collect();
    for chunk in ids.chunks(40) {
        let actions = chunk
            .iter()
            .flat_map(|id| {
                [
                    Action::PlaylistCreate {
                        playlist_id: *id,
                        name: "Equal book name".into(),
                    },
                    Action::AudiobookCreate { playlist_id: *id },
                ]
            })
            .collect();
        library.owner.sync(&library.app, actions).await;
    }
    let before = library.state().await;
    let path = "/api/v1/audiobooks/page?limit=60";
    let first = library
        .owner
        .request(&library.app, "GET", path, Value::Null)
        .await;
    assert_eq!(
        first.status,
        StatusCode::OK,
        "An additive paged route must make later books reachable"
    );
    assert_eq!(first.headers["cache-control"], "private, no-store");
    assert_eq!(
        first.body["items"],
        json!(
            ids[..60]
                .iter()
                .map(|id| json!({"id": id, "name": "Equal book name", "kind": "local"}))
                .collect::<Vec<_>>()
        )
    );
    let next = first.body["pagination"]["next_cursor"].as_str().unwrap();
    let second = library
        .owner
        .get(&library.app, &format!("{path}&cursor={next}"))
        .await;
    assert_eq!(
        second["items"],
        json!(
            ids[60..120]
                .iter()
                .map(|id| json!({"id": id, "name": "Equal book name", "kind": "local"}))
                .collect::<Vec<_>>()
        )
    );
    let last = library
        .owner
        .get(
            &library.app,
            &format!(
                "{path}&cursor={}",
                second["pagination"]["next_cursor"].as_str().unwrap()
            ),
        )
        .await;
    assert_eq!(last["items"][0]["id"], json!(ids[120]));
    assert_eq!(last["items"].as_array().unwrap().len(), 1);
    assert!(last["pagination"]["next_cursor"].is_null());
    let back = library
        .owner
        .get(
            &library.app,
            &format!(
                "{path}&cursor={}",
                last["pagination"]["previous_cursor"].as_str().unwrap()
            ),
        )
        .await;
    assert_eq!(back["items"], second["items"]);
    let index = library
        .owner
        .page(&library.app, "/audiobooks?limit=60")
        .await;
    assert_eq!(index.status, StatusCode::OK);
    assert_eq!(index.body["props"]["books"], first.body["items"]);
    assert_eq!(
        index.body["props"]["book_pagination"],
        first.body["pagination"]
    );
    let grouping = index.body["props"]["grouping_playlists"]
        .as_array()
        .unwrap();
    assert_eq!(grouping.len(), 2);
    assert!(
        grouping
            .iter()
            .all(|item| !ids.iter().any(|id| item["id"] == json!(id))),
        "Books on every page remain excluded from grouping choices"
    );
    let next_index = library
        .owner
        .page(&library.app, &format!("/audiobooks?cursor={next}&limit=60"))
        .await;
    assert_eq!(next_index.status, StatusCode::OK);
    assert_eq!(next_index.body["props"]["books"], second["items"]);
    assert_eq!(
        next_index.body["props"]["grouping_playlists"],
        index.body["props"]["grouping_playlists"]
    );
    let legacy = library.owner.get(&library.app, "/api/v1/audiobooks").await;
    assert_eq!(
        legacy.as_array().unwrap().len(),
        100,
        "The existing vector route remains wire compatible"
    );
    let peer = library
        .peer
        .request(
            &library.app,
            "GET",
            &format!("{path}&cursor={next}"),
            Value::Null,
        )
        .await;
    assert_eq!(
        peer.status,
        StatusCode::NOT_FOUND,
        "A cursor belongs only to its current account"
    );
    assert_eq!(
        library.state().await,
        before,
        "Paged reads preserve both accounts, original media, events, sources and explicit download absence"
    );
    library.assert_original_unchanged();
}

#[tokio::test]
async fn live_audiobook_pages_validate_cursors_and_restart_after_anchor_changes() {
    let library = Library::prepare().await;
    let mut actions = Vec::new();
    for (index, name) in [(200, "Z"), (201, "A"), (202, "A"), (203, "M"), (204, "B")] {
        let id = Uuid::from_u128(index);
        actions.extend([
            Action::PlaylistCreate {
                playlist_id: id,
                name: name.into(),
            },
            Action::AudiobookCreate { playlist_id: id },
        ]);
    }
    library.owner.sync(&library.app, actions).await;
    let path = "/api/v1/audiobooks/page?limit=2";
    let first = library.owner.get(&library.app, path).await;
    let ids = |page: &Value| {
        page["items"]
            .as_array()
            .unwrap()
            .iter()
            .map(|book| book["id"].clone())
            .collect::<Vec<_>>()
    };
    assert_eq!(
        ids(&first),
        json!([Uuid::from_u128(201), Uuid::from_u128(202)])
            .as_array()
            .unwrap()
            .clone()
    );
    let cursor = first["pagination"]["next_cursor"].as_str().unwrap();
    library
        .owner
        .sync(
            &library.app,
            vec![Action::PlaylistRename {
                playlist_id: Uuid::from_u128(202),
                name: "ZZ renamed".into(),
            }],
        )
        .await;
    let stale = library
        .owner
        .request(
            &library.app,
            "GET",
            &format!("{path}&cursor={cursor}"),
            Value::Null,
        )
        .await;
    assert_eq!(
        stale.status,
        StatusCode::CONFLICT,
        "A renamed cursor anchor requires an explicit restart"
    );
    let first = library.owner.get(&library.app, path).await;
    assert_eq!(
        ids(&first),
        json!([Uuid::from_u128(201), Uuid::from_u128(204)])
            .as_array()
            .unwrap()
            .clone()
    );
    let cursor = first["pagination"]["next_cursor"].as_str().unwrap();
    library
        .owner
        .sync(
            &library.app,
            vec![Action::PlaylistDelete {
                playlist_id: Uuid::from_u128(204),
            }],
        )
        .await;
    assert_eq!(
        library
            .owner
            .request(
                &library.app,
                "GET",
                &format!("{path}&cursor={cursor}"),
                Value::Null
            )
            .await
            .status,
        StatusCode::CONFLICT
    );
    let first = library.owner.get(&library.app, path).await;
    assert_eq!(
        ids(&first),
        json!([Uuid::from_u128(201), Uuid::from_u128(203)])
            .as_array()
            .unwrap()
            .clone()
    );
    let cursor = first["pagination"]["next_cursor"].as_str().unwrap();
    library
        .owner
        .sync(
            &library.app,
            vec![
                Action::PlaylistCreate {
                    playlist_id: Uuid::from_u128(205),
                    name: "Y concurrent".into(),
                },
                Action::AudiobookCreate {
                    playlist_id: Uuid::from_u128(205),
                },
            ],
        )
        .await;
    let before = library.state().await;
    let next = library
        .owner
        .get(&library.app, &format!("{path}&cursor={cursor}"))
        .await;
    assert_eq!(
        ids(&next),
        json!([Uuid::from_u128(205), Uuid::from_u128(200)])
            .as_array()
            .unwrap()
            .clone(),
        "A live continuation includes later additions without guessing a snapshot"
    );
    for query in [
        "limit=0".to_owned(),
        "limit=101".into(),
        "cursor=".into(),
        "cursor=not-a-valid-cursor".into(),
        format!("cursor={}", "x".repeat(4097)),
    ] {
        assert_eq!(
            library
                .owner
                .request(
                    &library.app,
                    "GET",
                    &format!("/api/v1/audiobooks/page?{query}"),
                    Value::Null
                )
                .await
                .status,
            StatusCode::UNPROCESSABLE_ENTITY
        );
    }
    assert_eq!(
        library
            .owner
            .request(
                &library.app,
                "GET",
                "/api/v1/audiobooks/page?limit=65536",
                Value::Null
            )
            .await
            .status,
        StatusCode::BAD_REQUEST
    );
    assert_eq!(
        library
            .owner
            .get_claiming(&library.app, path, &library.peer)
            .await
            .status,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        library.state().await,
        before,
        "Invalid and live page reads preserve current private musical state and download absence"
    );
    library.assert_original_unchanged();
}

#[tokio::test]
async fn every_accepted_playlist_name_round_trips_next_and_previous_book_cursors() {
    let library = Library::prepare().await;
    let mut actions = Vec::new();
    for (index, name) in [
        (601, "\u{1}".repeat(256)),
        (602, "\u{1}".repeat(256)),
        (603, "Adjacent owned book".into()),
    ] {
        let id = Uuid::from_u128(index);
        actions.extend([
            Action::PlaylistCreate {
                playlist_id: id,
                name,
            },
            Action::AudiobookCreate { playlist_id: id },
        ]);
    }
    library.owner.sync(&library.app, actions).await;
    let before = library.state().await;
    let path = "/api/v1/audiobooks/page?limit=1";
    let first = library.owner.get(&library.app, path).await;
    assert_eq!(first["items"][0]["id"], json!(Uuid::from_u128(601)));
    let cursor = first["pagination"]["next_cursor"].as_str().unwrap();
    assert!(cursor.len() > 2048 && cursor.len() <= 4096);
    let second = library
        .owner
        .request(
            &library.app,
            "GET",
            &format!("{path}&cursor={cursor}"),
            Value::Null,
        )
        .await;
    assert_eq!(
        second.status,
        StatusCode::OK,
        "An accepted name must never produce an unusable next cursor"
    );
    assert_eq!(second.body["items"][0]["id"], json!(Uuid::from_u128(602)));
    let previous = second.body["pagination"]["previous_cursor"]
        .as_str()
        .unwrap();
    let back = library
        .owner
        .request(
            &library.app,
            "GET",
            &format!("{path}&cursor={previous}"),
            Value::Null,
        )
        .await;
    assert_eq!(
        back.status,
        StatusCode::OK,
        "An accepted name must never produce an unusable previous cursor"
    );
    assert_eq!(back.body["items"], first["items"]);
    let next = second.body["pagination"]["next_cursor"].as_str().unwrap();
    let last = library
        .owner
        .get(&library.app, &format!("{path}&cursor={next}"))
        .await;
    assert_eq!(last["items"][0]["id"], json!(Uuid::from_u128(603)));
    assert!(last["pagination"]["next_cursor"].is_null());
    assert_eq!(library.state().await, before);
    library.assert_original_unchanged();
}
