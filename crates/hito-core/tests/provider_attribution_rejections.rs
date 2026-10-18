#[path = "fixtures/content_service.rs"]
mod fixture;
mod support;

use hito_core::{
    Action, Actor, AdoptProviderRecording, Credentials, Error, InstallPlugin, OfferPluginService,
    Recording, Services,
};
use serde_json::{Value, json};
use std::future::pending;
use uuid::Uuid;

#[tokio::test]
async fn rejected_attribution_cannot_publish_partial_state_or_cross_account_boundaries() {
    let server = fixture::ProviderServer::start().await;
    let (_directory, hito, actor, _shared_library, music) =
        support::library("provider-attribution-rejections").await;
    let original_path = music.join("original.wav");
    let original = support::write_wav(&original_path, 435);
    let mut detail = json!({
        "id": "recording/one",
        "title": "Provider title",
        "artist": "Provider artist",
        "duration_ms": 1000,
        "attribution": {
            "creators": ["Owned creator"],
            "provider_label": "Owned catalog",
            "content_url": "https://owned-user:owned-secret@catalog.example/recordings/one"
        }
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
    assert_rejected(
        hito.catalog
            .adopt_provider(&actor, &installation.id, request())
            .await,
    );
    let catalog = hito
        .catalog
        .recordings(&actor, Default::default())
        .await
        .unwrap();
    assert!(catalog.total == 0 && catalog.items.is_empty());
    assert!(
        hito.collection
            .activity(&actor)
            .await
            .unwrap()
            .history
            .is_empty()
    );
    assert!(
        hito.collection
            .events(&actor, 0)
            .await
            .unwrap()
            .operations
            .is_empty()
    );
    assert!(!hito.downloads.run_next_job_until(pending()).await.unwrap());
    assert!(server.requests("v1/audio").await.is_empty());

    detail["attribution"]["content_url"] = json!("https://catalog.example/recordings/one");
    server.reply("v1/recordings/detail", detail.clone()).await;
    let adopted = hito
        .catalog
        .adopt_provider(&actor, &installation.id, request())
        .await
        .unwrap();
    let metadata = hito
        .catalog
        .recording_metadata(&actor, &adopted.id, Default::default())
        .await
        .unwrap();
    assert_eq!(metadata.observations.items.len(), 1);
    assert_eq!(
        serde_json::to_value(metadata.selected_observation.unwrap()).unwrap()["attribution"],
        detail["attribution"]
    );
    let recording_id = Uuid::parse_str(&adopted.id).unwrap();
    let playlist_id = Uuid::new_v4();
    support::synchronize(
        &hito,
        &actor,
        vec![
            Action::PlaylistCreate {
                playlist_id,
                name: "Private accepted recording".into(),
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
    detail["attribution"]["content_url"] =
        json!("https://catalog.example/recordings/one?signature=owned-secret");
    server.reply("v1/recordings/detail", detail).await;
    assert_rejected(
        hito.catalog
            .adopt_provider(&actor, &installation.id, request())
            .await,
    );
    assert_eq!(
        snapshot(&hito, &actor, &adopted.id, &playlist).await,
        before,
        "Rejected replacement credits must preserve all accepted metadata and private state"
    );
    assert!(matches!(
        hito.downloads.status(&actor, &adopted.id).await,
        Err(Error::NotFound)
    ));
    assert!(!hito.downloads.run_next_job_until(pending()).await.unwrap());

    let credentials = || Credentials {
        username: "other-attribution-account".into(),
        password: "owned attribution account password".into(),
    };
    hito.identity
        .create_user(&actor, credentials())
        .await
        .unwrap();
    let session = hito.identity.login(credentials()).await.unwrap();
    let other = hito.identity.authenticate(&session.token).await.unwrap();
    let requests = request_counts(&server).await;
    assert_eq!(
        hito.catalog
            .recordings(&other, Default::default())
            .await
            .unwrap()
            .total,
        0
    );
    assert!(
        hito.plugins
            .settings(&other)
            .await
            .unwrap()
            .installations
            .is_empty()
    );
    assert!(matches!(
        hito.catalog
            .recording_metadata(&other, &adopted.id, Default::default())
            .await,
        Err(Error::NotFound)
    ));
    assert!(matches!(
        hito.catalog.sources(&other, &adopted.id).await,
        Err(Error::NotFound)
    ));
    assert!(matches!(
        hito.catalog.source_choice(&other, &adopted.id).await,
        Err(Error::NotFound)
    ));
    assert!(matches!(
        hito.collection.playlist(&other, &playlist).await,
        Err(Error::NotFound)
    ));
    assert!(matches!(
        hito.downloads.status(&other, &adopted.id).await,
        Err(Error::NotFound)
    ));
    assert!(matches!(
        hito.catalog
            .adopt_provider(&other, &installation.id, request())
            .await,
        Err(Error::NotFound)
    ));
    assert_eq!(request_counts(&server).await, requests);
    assert_eq!(
        snapshot(&hito, &actor, &adopted.id, &playlist).await,
        before
    );
    assert!(server.requests("v1/audio").await.is_empty());
    assert!(!hito.downloads.run_next_job_until(pending()).await.unwrap());
    assert_eq!(std::fs::read(original_path).unwrap(), original);
    server.shutdown().await;
}

fn assert_rejected(result: hito_core::Result<Recording>) {
    let error = match result {
        Ok(_) => panic!("Invalid provider attribution must be rejected before publication"),
        Err(error) => error,
    };
    assert_eq!(error.code(), "upstream");
    assert_eq!(
        error.to_string(),
        "The provider returned invalid recording details"
    );
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
