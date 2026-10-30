use super::library::Library;
use hito_core::{Action, Operation, SyncBatch};
use serde_json::{Value, json};
use uuid::Uuid;

#[derive(Clone, Copy)]
pub struct Marker {
    pub id: Uuid,
    pub audiobook_id: Uuid,
    pub entry_id: Uuid,
    pub recording_id: Uuid,
    pub position_ms: i64,
    pub label: &'static str,
}

impl Marker {
    pub fn create(self, operation_id: Uuid) -> Operation {
        Operation {
            id: operation_id,
            action: Action::AudiobookBookmarkCreate {
                audiobook_id: self.audiobook_id,
                bookmark_id: self.id,
                entry_id: self.entry_id,
                recording_id: self.recording_id,
                position_ms: self.position_ms,
                label: self.label.into(),
            },
        }
    }
}

pub fn removal(audiobook_id: Uuid, bookmark_id: Uuid, operation_id: Uuid) -> Operation {
    Operation {
        id: operation_id,
        action: Action::AudiobookBookmarkRemove {
            audiobook_id,
            bookmark_id,
        },
    }
}

pub fn batch(operation: Operation) -> SyncBatch {
    SyncBatch {
        device_id: "audiobook-bookmark-workflow".into(),
        operations: vec![operation],
    }
}

pub async fn accept(library: &Library, operation: Operation) {
    let id = operation.id;
    let receipt = library
        .hito
        .collection
        .apply(&library.actor, batch(operation))
        .await
        .unwrap();
    assert!(
        receipt.acknowledged.as_slice() == [id],
        "The explicit bookmark operation must retain its exact acknowledgement"
    );
}

pub async fn snapshot(library: &Library, alternate: Uuid) -> Value {
    json!({
        "book": library.hito.collection.audiobook(&library.actor, &library.playlist.to_string()).await.unwrap(),
        "alternate": library.hito.collection.audiobook(&library.actor, &alternate.to_string()).await.unwrap(),
        "musical": library.snapshot().await,
        "events": library.hito.collection.events(&library.actor, 0).await.unwrap(),
    })
}

pub fn assert_visible_unchanged(before: &Value, after: &Value) {
    assert!(
        after["book"] == before["book"]
            && after["alternate"] == before["alternate"]
            && after["musical"] == before["musical"],
        "A deletion-winning operation must not change visible bookmarks, either book or existing musical/private state"
    );
}

pub fn assert_event_added(before: &Value, after: &Value, operation_id: Uuid) {
    let old = before["events"]["operations"].as_array().unwrap();
    let new = after["events"]["operations"].as_array().unwrap();
    assert!(
        new.len() == old.len() + 1
            && new[..old.len()] == old[..]
            && new.last().unwrap()["operation"]["id"] == operation_id.to_string(),
        "A fresh accepted operation must add exactly one event and preserve the accepted prefix"
    );
}
