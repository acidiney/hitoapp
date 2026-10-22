use hito_core::test_support::content_service as fixture;
use hito_core::test_support::support;

use hito_core::{
    Action, Actor, AdoptProviderRecording, ChooseSource, CoreConfig, Error, HostDeviceId,
    InstallPlugin, OfferPluginService, Services, open,
};
use serde_json::{Value, json};
use std::future::pending;
use uuid::Uuid;

#[tokio::test]
async fn provider_attribution_survives_personal_edits_and_reopening_without_fetching_audio() {
    let server = fixture::ProviderServer::start().await;
    let (directory, hito, actor, shared_library, music) =
        support::library("provider-attribution").await;
    let original_path = music.join("original.wav");
    let original = support::write_wav(&original_path, 431);
    let attribution = json!({
        "creators": ["Owned lead creator", "Owned guest creator"],
        "provider_label": "Owned catalog",
        "content_url": "https://catalog.example/recordings/one"
    });
    server
        .reply(
            "v1/recordings/detail",
            json!({
                "id": "recording/one",
                "title": "Provider title",
                "artist": "Provider artist",
                "duration_ms": 1000,
                "revision": "one",
                "attribution": attribution
            }),
        )
        .await;
    server
        .reply(
            "v1/sources",
            json!({"items": [{
                "id": "audio/one",
                "label": "Owned audio",
                "ready": true
            }]}),
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
    let adopted = hito
        .catalog
        .adopt_provider(
            &actor,
            &installation.id,
            AdoptProviderRecording {
                installation_revision: installation.revision,
                provider_recording_id: "recording/one".into(),
            },
        )
        .await
        .unwrap();
    assert_ne!(adopted.library_id, shared_library);
    assert_eq!(adopted.title, "Provider title");
    assert_eq!(adopted.artist, "Provider artist");
    let metadata = serde_json::to_value(
        hito.catalog
            .recording_metadata(&actor, &adopted.id, Default::default())
            .await
            .unwrap(),
    )
    .unwrap();
    let observation = &metadata["selected_observation"];
    assert_eq!(observation["provider"], "org.hito.content-fixture");
    assert_eq!(observation["provider_recording_id"], "recording/one");
    assert_eq!(observation["plugin_installation_id"], installation.id);
    assert_eq!(observation["attribution"], attribution);
    assert_eq!(metadata["observations"]["items"][0], *observation);
    assert_eq!(
        metadata["observations"]["items"].as_array().unwrap().len(),
        1
    );

    let sources = hito.catalog.sources(&actor, &adopted.id).await.unwrap();
    assert_eq!(sources.len(), 1);
    assert!(sources[0].ready);
    let recording_id = Uuid::parse_str(&adopted.id).unwrap();
    let playlist_id = Uuid::new_v4();
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
    support::synchronize(
        &hito,
        &actor,
        vec![
            Action::PlaylistCreate {
                playlist_id,
                name: "Private attributed playlist".into(),
            },
            Action::EntryInsert {
                playlist_id,
                entry_id: Uuid::new_v4(),
                recording_id,
                position: 0,
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
    let before = snapshot(&hito, &actor, &adopted.id, &playlist).await;
    assert_eq!(before["metadata"]["title"]["value"], "My private title");
    assert_eq!(before["metadata"]["artist"]["value"], "My private artist");
    assert_eq!(before["metadata"]["title"]["inherited"], "Provider title");
    assert_eq!(before["metadata"]["artist"]["inherited"], "Provider artist");
    assert_eq!(before["metadata"]["selected_observation"], *observation);
    assert_eq!(
        snapshot(&hito, &actor, &adopted.id, &playlist).await,
        before,
        "Reading retained credits must preserve catalog, source choice and private state"
    );
    assert!(server.requests("v1/audio").await.is_empty());
    assert!(matches!(
        hito.downloads.status(&actor, &adopted.id).await,
        Err(Error::NotFound)
    ));
    assert!(!hito.downloads.run_next_job_until(pending()).await.unwrap());

    drop(hito);
    let reopened = open(CoreConfig::new(
        directory.path().join("hito.db"),
        music,
        HostDeviceId::new("provider-attribution").unwrap(),
    ))
    .await
    .unwrap();
    assert_eq!(
        snapshot(&reopened, &actor, &adopted.id, &playlist).await,
        before,
        "Reopening core must retain the exact credits, identities and private state"
    );
    assert!(matches!(
        reopened.downloads.status(&actor, &adopted.id).await,
        Err(Error::NotFound)
    ));
    assert!(
        !reopened
            .downloads
            .run_next_job_until(pending())
            .await
            .unwrap()
    );
    assert!(server.requests("v1/audio").await.is_empty());
    assert_eq!(std::fs::read(original_path).unwrap(), original);
    server.shutdown().await;
}

async fn snapshot(hito: &Services, actor: &Actor, recording: &str, playlist: &str) -> Value {
    json!({
        "catalog": hito.catalog.recordings(actor, Default::default()).await.unwrap(),
        "sources": hito.catalog.sources(actor, recording).await.unwrap(),
        "source_choice": hito.catalog.source_choice(actor, recording).await.unwrap(),
        "playlist": hito.collection.playlist(actor, playlist).await.unwrap(),
        "activity": hito.collection.activity(actor).await.unwrap(),
        "events": hito.collection.events(actor, 0).await.unwrap(),
        "metadata": hito.catalog.recording_metadata(actor, recording, Default::default()).await.unwrap()
    })
}
