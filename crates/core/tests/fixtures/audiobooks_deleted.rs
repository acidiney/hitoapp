use super::library::Library;
use hito_core::{Action, Actor, Browse, Error, MetadataHistoryQuery, Operation, Search, SyncBatch};
use serde_json::{Value, json};
use uuid::Uuid;

pub async fn musical(library: &Library, unrelated: Uuid) -> Value {
    let mut metadata = Vec::new();
    let mut sources = Vec::new();
    for recording in library.recordings {
        metadata.push(
            library
                .hito
                .catalog
                .recording_metadata(
                    &library.actor,
                    &recording.to_string(),
                    MetadataHistoryQuery::default(),
                )
                .await
                .unwrap(),
        );
        sources.push(
            library
                .hito
                .catalog
                .sources(&library.actor, &recording.to_string())
                .await
                .unwrap(),
        );
    }
    json!({
        "libraries": library.hito.catalog.libraries(&library.actor).await.unwrap(),
        "recordings": library.hito.catalog.recordings(&library.actor, Search::default()).await.unwrap(),
        "artists": library.hito.catalog.artists(&library.actor, Browse::default()).await.unwrap(),
        "releases": library.hito.catalog.releases(&library.actor, Browse::default()).await.unwrap(),
        "metadata": metadata.iter().map(hito_core::test_support::support::metadata_snapshot).collect::<Vec<_>>(),
        "sources": sources,
        "playlists": library.hito.collection.playlists(&library.actor).await.unwrap(),
        "unrelated": library.hito.collection.playlist(&library.actor, &unrelated.to_string()).await.unwrap(),
        "activity": library.hito.collection.activity(&library.actor).await.unwrap(),
        "progress": library.hito.collection.saved_playback(&library.actor, &library.recordings).await.unwrap(),
    })
}

pub async fn events(library: &Library) -> Value {
    serde_json::to_value(
        library
            .hito
            .collection
            .events(&library.actor, 0)
            .await
            .unwrap(),
    )
    .unwrap()
}

pub async fn accept(library: &Library, actions: Vec<Action>) {
    let before = events(library).await;
    let operations: Vec<Operation> = actions
        .into_iter()
        .map(|action| Operation {
            id: Uuid::new_v4(),
            action,
        })
        .collect();
    let ids: Vec<Uuid> = operations.iter().map(|operation| operation.id).collect();
    let payloads = serde_json::to_value(&operations).unwrap();
    let receipt = library
        .hito
        .collection
        .apply(
            &library.actor,
            SyncBatch {
                device_id: "audiobook-deletion-workflow".into(),
                operations,
            },
        )
        .await
        .expect("Owned deleted-book operations must not reject independent work in the same batch");
    assert!(
        receipt.acknowledged == ids,
        "Every operation in the mixed deletion workflow must retain its exact ordered acknowledgement"
    );
    let after = events(library).await;
    let old = before["operations"].as_array().unwrap();
    let new = after["operations"].as_array().unwrap();
    assert!(
        new.len() == old.len() + ids.len() && new[..old.len()] == old[..],
        "The accepted batch must append one event per operation without changing its prior history"
    );
    for (event, operation) in new[old.len()..].iter().zip(payloads.as_array().unwrap()) {
        assert!(
            event["device_id"] == "audiobook-deletion-workflow" && event["operation"] == *operation,
            "Deletion-winning no-ops and independent edits must retain their exact original sync payloads"
        );
    }
    assert!(
        after["has_more"] == false && after["cursor"] == new.last().unwrap()["sequence"],
        "The bounded workflow must retain the exact accepted terminal cursor"
    );
}

pub async fn assert_deleted(library: &Library) {
    let id = library.playlist.to_string();
    assert!(
        matches!(
            library.hito.collection.playlist(&library.actor, &id).await,
            Err(Error::NotFound)
        ),
        "A deleted backing playlist must remain absent from its public detail capability"
    );
    assert!(
        matches!(
            library.hito.collection.audiobook(&library.actor, &id).await,
            Err(Error::NotFound)
        ),
        "No delayed grouping or bookmark operation may resurrect the deleted audiobook or expose its markers"
    );
}

pub fn expected_after_deletion(before: &Value, book: Uuid, unrelated: Uuid, name: &str) -> Value {
    let mut expected = before.clone();
    forget_playlist(&mut expected, book);
    rename(&mut expected, unrelated, name);
    expected
}

pub fn forget_playlist(expected: &mut Value, playlist: Uuid) {
    expected["playlists"]
        .as_array_mut()
        .unwrap()
        .retain(|entry| entry["id"] != playlist.to_string());
    for metadata in expected["metadata"].as_array_mut().unwrap() {
        metadata["appears_in"]
            .as_array_mut()
            .unwrap()
            .retain(|entry| entry["id"] != playlist.to_string());
    }
}

pub fn rename(expected: &mut Value, unrelated: Uuid, name: &str) {
    let playlist = expected["playlists"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|playlist| playlist["id"] == unrelated.to_string())
        .unwrap();
    playlist["name"] = json!(name);
    expected["unrelated"]["playlist"]["name"] = json!(name);
    for metadata in expected["metadata"].as_array_mut().unwrap() {
        for appearance in metadata["appears_in"].as_array_mut().unwrap() {
            if appearance["id"] == unrelated.to_string() {
                appearance["name"] = json!(name);
            }
        }
    }
}

pub async fn deny(library: &Library, actor: &Actor, action: Action) {
    let result = library
        .hito
        .collection
        .apply(
            actor,
            SyncBatch {
                device_id: "audiobook-deletion-workflow".into(),
                operations: vec![Operation {
                    id: Uuid::new_v4(),
                    action,
                }],
            },
        )
        .await;
    assert!(
        matches!(result, Err(Error::NotFound)),
        "A foreign deleted book must not authorize a no-op or reveal its retained private state"
    );
}
