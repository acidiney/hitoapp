use super::owner;
use hito_core::{Action, Actor, Services};
use serde_json::{Value, json};
use uuid::Uuid;

pub async fn seed(services: &Services, actor: &Actor, playlist_id: Uuid) -> String {
    let library = services
        .catalog
        .libraries(actor)
        .await
        .unwrap()
        .remove(0)
        .id;
    hito_core::test_support::support::scan(services, actor, &library).await;
    let recordings = services
        .catalog
        .recordings(actor, Default::default())
        .await
        .unwrap();
    assert_eq!(recordings.total, 1);
    let recording = recordings.items[0].id.clone();
    let recording_id = recording.parse().unwrap();
    hito_core::test_support::support::synchronize(
        services,
        actor,
        vec![
            Action::EntryInsert {
                playlist_id,
                entry_id: Uuid::new_v4(),
                recording_id,
                position: 0,
            },
            Action::LikeSet {
                recording_id,
                liked: true,
            },
            Action::ProgressSet {
                recording_id,
                position_ms: 500,
                speed: 1.25,
            },
        ],
    )
    .await;
    recording
}

pub async fn snapshot(
    services: &Services,
    actor: &Actor,
    proof: &owner::Proof,
    recording: &str,
) -> Value {
    let recordings = services
        .catalog
        .recordings(actor, Default::default())
        .await
        .unwrap();
    assert_eq!(
        recordings.total, 1,
        "remote facts fabricated ordinary recordings"
    );
    assert_eq!(recordings.items[0].id, recording);
    let sources = services.catalog.sources(actor, recording).await.unwrap();
    assert_eq!(
        sources.len(),
        1,
        "remote observations fabricated ordinary sources"
    );
    assert!(sources[0].ready);
    let liked = services
        .catalog
        .recording_liked(actor, recording)
        .await
        .unwrap();
    assert!(liked);
    let progress = services
        .collection
        .saved_playback(actor, &[recording.parse().unwrap()])
        .await
        .unwrap();
    assert_eq!(progress.len(), 1);
    assert_eq!(progress[0].position_ms, 500);
    assert_eq!(progress[0].speed, 1.25);
    let metadata = services
        .catalog
        .recording_metadata(actor, recording, Default::default())
        .await
        .unwrap();
    let playlist = services
        .collection
        .playlist(actor, &proof.standalone.to_string())
        .await
        .unwrap();
    assert_eq!(playlist.entries.len(), 1);
    let events = services.collection.events(actor, 0).await.unwrap();
    assert_eq!(events.operations.len(), 4);
    json!({"recordings":recordings,"sources":sources,"metadata":metadata,"playlist":playlist,"events":events,"liked":liked,"progress":progress})
}
