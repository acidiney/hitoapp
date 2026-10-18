#[path = "fixtures/content_service.rs"]
mod fixture;
mod support;

use hito_core::{
    Action, Actor, AdoptProviderRecording, ChooseSource, CoreConfig, HostDeviceId, InstallPlugin,
    OfferPluginService, Services, open,
};
use serde::Serialize;
use serde_json::{Value, json};
use uuid::Uuid;

#[tokio::test]
async fn adding_provider_credits_retains_unattributed_history_and_private_state_after_disconnect() {
    let server = fixture::ProviderServer::start().await;
    let (directory, hito, actor, shared_library, music) =
        support::library("provider-attribution-history").await;
    let original_path = music.join("original.wav");
    let original = support::write_wav(&original_path, 433);
    let mut detail = json!({
        "id": "recording/one",
        "title": "Provider title",
        "artist": "Provider artist",
        "duration_ms": 1000,
        "revision": "unchanged-provider-revision"
    });
    server.reply("v1/recordings/detail", detail.clone()).await;
    server
        .reply(
            "v1/sources",
            json!({"items": [{"id": "audio/one", "label": "Owned audio", "ready": true}]}),
        )
        .await;
    let service = hito
        .plugins
        .offer_service(
            &actor,
            OfferPluginService {
                name: "Owned catalog".into(),
                base_url: server.url.clone(),
            },
        )
        .await
        .unwrap();
    let installation = hito
        .plugins
        .install(
            &actor,
            InstallPlugin {
                accept_manifest_digest: None,
                service_id: service.id,
                service_revision: service.revision,
                bearer_token: None,
            },
        )
        .await
        .unwrap();
    let request = || AdoptProviderRecording {
        installation_revision: installation.revision,
        provider_recording_id: "recording/one".into(),
    };
    let adopted = hito
        .catalog
        .adopt_provider(&actor, &installation.id, request())
        .await
        .unwrap();
    assert_ne!(adopted.library_id, shared_library);
    let initial = hito
        .catalog
        .recording_metadata(&actor, &adopted.id, Default::default())
        .await
        .unwrap();
    let previous = initial.selected_observation.as_ref().unwrap();
    let previous_bytes = serde_json::to_vec(previous).unwrap();
    let previous_history = serde_json::to_vec(&initial.observations).unwrap();
    assert!(
        serde_json::to_value(previous)
            .unwrap()
            .get("attribution")
            .is_none()
    );
    assert_eq!(initial.observations.items.len(), 1);

    let sources = hito.catalog.sources(&actor, &adopted.id).await.unwrap();
    assert_eq!(sources.len(), 1);
    hito.catalog
        .choose_source(
            &actor,
            &adopted.id,
            ChooseSource {
                request_id: Uuid::new_v4(),
                source_id: Uuid::parse_str(&sources[0].id).unwrap(),
            },
        )
        .await
        .unwrap();
    let recording_id = Uuid::parse_str(&adopted.id).unwrap();
    let playlist_id = Uuid::new_v4();
    support::synchronize(
        &hito,
        &actor,
        vec![
            Action::PlaylistCreate {
                playlist_id,
                name: "Private repeated recording".into(),
            },
            Action::EntryInsert {
                playlist_id,
                entry_id: Uuid::new_v4(),
                recording_id,
                position: 0,
            },
            Action::EntryInsert {
                playlist_id,
                entry_id: Uuid::new_v4(),
                recording_id,
                position: 1,
            },
            Action::RecordingTitleSet {
                recording_id,
                title: "My private title".into(),
            },
            Action::RecordingArtistSet {
                recording_id,
                artist: "My private artist".into(),
            },
            Action::LikeSet {
                recording_id,
                liked: true,
            },
            Action::ProgressSet {
                recording_id,
                position_ms: 700,
                speed: 1.25,
            },
        ],
    )
    .await;
    let playlist = playlist_id.to_string();
    let private = private_state(&hito, &actor, &adopted.id, &playlist).await;
    assert_eq!(private["entries"].as_array().unwrap().len(), 2);
    let repeated = hito
        .catalog
        .adopt_provider(&actor, &installation.id, request())
        .await
        .unwrap();
    assert_eq!(
        (repeated.id, repeated.library_id),
        (adopted.id.clone(), adopted.library_id.clone())
    );
    let unchanged = hito
        .catalog
        .recording_metadata(&actor, &adopted.id, Default::default())
        .await
        .unwrap();
    assert_eq!(
        serde_json::to_vec(&unchanged.observations).unwrap(),
        previous_history
    );
    assert_eq!(
        unchanged.selected_observation.as_ref().unwrap().id,
        previous.id
    );

    let attribution = json!({
        "creators": ["Owned lead creator", "Owned guest creator"],
        "provider_label": "Owned catalog",
        "content_url": "https://catalog.example/recordings/one"
    });
    detail["attribution"] = attribution.clone();
    server.reply("v1/recordings/detail", detail).await;
    let refreshed = hito
        .catalog
        .adopt_provider(&actor, &installation.id, request())
        .await
        .unwrap();
    assert_eq!(
        (refreshed.id, refreshed.library_id),
        (adopted.id.clone(), adopted.library_id.clone())
    );
    let credited = hito
        .catalog
        .recording_metadata(&actor, &adopted.id, Default::default())
        .await
        .unwrap();
    let current = credited.selected_observation.as_ref().unwrap();
    assert_ne!(current.id, previous.id);
    assert_eq!(
        serde_json::to_value(current).unwrap()["attribution"],
        attribution
    );
    assert_eq!(credited.observations.items.len(), 2);
    assert_eq!(
        serde_json::to_vec(&credited.observations.items[1]).unwrap(),
        previous_bytes
    );
    let credited_bytes = serde_json::to_vec(&credited).unwrap();
    hito.catalog
        .adopt_provider(&actor, &installation.id, request())
        .await
        .unwrap();
    assert_eq!(
        serde_json::to_vec(
            &hito
                .catalog
                .recording_metadata(&actor, &adopted.id, Default::default())
                .await
                .unwrap()
        )
        .unwrap(),
        credited_bytes,
        "An identical attributed payload must reuse its immutable observation"
    );
    assert_eq!(
        private_state(&hito, &actor, &adopted.id, &playlist).await,
        private
    );
    let requests = request_counts(&server).await;
    assert_eq!(requests[1..], [4, 4, 0]);
    server.offline().await;
    hito.plugins
        .remove(&actor, &installation.id, installation.revision)
        .await
        .unwrap();
    assert_eq!(
        serde_json::to_vec(
            &hito
                .catalog
                .recording_metadata(&actor, &adopted.id, Default::default())
                .await
                .unwrap()
        )
        .unwrap(),
        credited_bytes
    );
    assert_eq!(
        private_state(&hito, &actor, &adopted.id, &playlist).await,
        private
    );
    assert!(!hito.catalog.sources(&actor, &adopted.id).await.unwrap()[0].ready);

    drop(hito);
    let reopened = open(CoreConfig::new(
        directory.path().join("hito.db"),
        music,
        HostDeviceId::new("provider-attribution-history").unwrap(),
    ))
    .await
    .unwrap();
    assert_eq!(
        serde_json::to_vec(
            &reopened
                .catalog
                .recording_metadata(&actor, &adopted.id, Default::default())
                .await
                .unwrap()
        )
        .unwrap(),
        credited_bytes,
        "Disconnected credits and the exact old observation must survive reopening"
    );
    assert_eq!(
        private_state(&reopened, &actor, &adopted.id, &playlist).await,
        private
    );
    assert_eq!(request_counts(&server).await, requests);
    assert_eq!(std::fs::read(original_path).unwrap(), original);
    server.shutdown().await;
}

async fn private_state(hito: &Services, actor: &Actor, recording: &str, playlist: &str) -> Value {
    let metadata = hito
        .catalog
        .recording_metadata(actor, recording, Default::default())
        .await
        .unwrap();
    let catalog = hito
        .catalog
        .recordings(actor, Default::default())
        .await
        .unwrap();
    let playlist = hito.collection.playlist(actor, playlist).await.unwrap();
    let activity = hito.collection.activity(actor).await.unwrap();
    let choice = hito.catalog.source_choice(actor, recording).await.unwrap();
    json!({
        "catalog": catalog.items.into_iter().map(retained_display).collect::<Vec<_>>(),
        "playlist": playlist.playlist,
        "entries": playlist.entries.into_iter().map(retained_display).collect::<Vec<_>>(),
        "activity": activity.history.into_iter().map(retained_display).collect::<Vec<_>>(),
        "events": hito.collection.events(actor, 0).await.unwrap(),
        "metadata": {"title": metadata.title, "artist": metadata.artist},
        "source_pin": {
            "revision": choice.revision,
            "mode": choice.mode,
            "sources": choice.sources.iter().map(|entry| json!({
                "id": entry.source.id, "label": entry.source.label, "selected": entry.selected
            })).collect::<Vec<_>>()
        }
    })
}

fn retained_display(value: impl Serialize) -> Value {
    let mut value = serde_json::to_value(value).unwrap();
    value.as_object_mut().unwrap().remove("available");
    value
}

async fn request_counts(server: &fixture::ProviderServer) -> Vec<usize> {
    let mut counts = Vec::new();
    for path in [
        "v1/manifest",
        "v1/recordings/detail",
        "v1/sources",
        "v1/audio",
    ] {
        counts.push(server.requests(path).await.len());
    }
    counts
}
