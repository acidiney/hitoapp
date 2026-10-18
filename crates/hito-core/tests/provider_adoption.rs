#[path = "fixtures/content_service.rs"]
mod fixture;
mod support;
use hito_core::{AdoptProviderRecording, InstallPlugin, OfferPluginService};

#[tokio::test]
async fn selected_provider_recordings_are_durable_private_and_keep_real_provenance() {
    let server = fixture::ProviderServer::start().await;
    let (dir, hito, alice, shared, _music) = support::library("provider-adoption").await;
    let service = hito
        .plugins
        .offer_service(
            &alice,
            OfferPluginService {
                name: "Fixture".into(),
                base_url: server.url.clone(),
            },
        )
        .await
        .unwrap();
    let installed = hito
        .plugins
        .install(
            &alice,
            InstallPlugin {
                accept_manifest_digest: None,
                service_id: service.id,
                service_revision: 1,
                bearer_token: Some("adoption-private-token".into()),
            },
        )
        .await
        .unwrap();
    server.reply("v1/recordings/detail", serde_json::json!({"id":"item/1","title":"Selected track","artist":"Provider artist","duration_ms":123000,"revision":"r1","release":{"id":"album/1","title":"Provider album","artist":"Provider artist"},"track_number":1})).await;
    server
        .reply(
            "v1/sources",
            serde_json::json!({"items":[{"id":"audio/1","label":"Original","ready":true}]}),
        )
        .await;
    let adopted = hito
        .catalog
        .adopt_provider(
            &alice,
            &installed.id,
            AdoptProviderRecording {
                installation_revision: 1,
                provider_recording_id: "item/1".into(),
            },
        )
        .await
        .unwrap();
    assert_ne!(adopted.library_id, shared);
    assert_eq!(adopted.title, "Selected track");
    assert_eq!(adopted.album.as_deref(), Some("Provider album"));
    assert!(adopted.available);
    assert!(uuid::Uuid::parse_str(&adopted.id).is_ok());
    let sources = hito.catalog.sources(&alice, &adopted.id).await.unwrap();
    assert_eq!(sources.len(), 1);
    assert!(sources[0].ready);
    let pool = sqlx::SqlitePool::connect_with(
        sqlx::sqlite::SqliteConnectOptions::new()
            .filename(dir.path().join("hito.db"))
            .foreign_keys(true),
    )
    .await
    .unwrap();
    let provenance: (String, Option<String>) = sqlx::query_as("SELECT m.provenance,r.initial_file_checksum FROM source_metadata m JOIN sources s ON s.id=m.source_id JOIN recordings r ON r.id=s.recording_id WHERE s.id=?")
        .bind(&sources[0].id).fetch_one(&pool).await.unwrap();
    assert_eq!(provenance, ("provider_response".into(), None));
    sqlx::query("UPDATE source_metadata SET provenance='file_tags' WHERE source_id=?")
        .bind(&sources[0].id)
        .execute(&pool)
        .await
        .unwrap();
    let again = hito
        .catalog
        .adopt_provider(
            &alice,
            &installed.id,
            AdoptProviderRecording {
                installation_revision: 1,
                provider_recording_id: "item/1".into(),
            },
        )
        .await
        .unwrap();
    assert_eq!(again.id, adopted.id);
    assert_eq!(
        sqlx::query_scalar::<_, String>("SELECT provenance FROM source_metadata WHERE source_id=?")
            .bind(&sources[0].id)
            .fetch_one(&pool)
            .await
            .unwrap(),
        "provider_response"
    );
    assert_eq!(
        hito.catalog.sources(&alice, &again.id).await.unwrap()[0].id,
        sources[0].id
    );
    let metadata = hito
        .catalog
        .recording_metadata(&alice, &adopted.id, Default::default())
        .await
        .unwrap();
    assert_eq!(metadata.observations.items.len(), 1);
    let observed = metadata.selected_observation.unwrap();
    assert!(matches!(
        observed.origin,
        hito_core::ObservationOrigin::ProviderResponse
    ));
    assert!(observed.source_id.is_none());
    assert!(observed.inspection_version.is_none());
    assert_eq!(observed.provider, "org.hito.content-fixture");
    assert_eq!(observed.provider_recording_id.as_deref(), Some("item/1"));
    assert_eq!(
        observed.plugin_installation_id.as_deref(),
        Some(installed.id.as_str())
    );
    assert_eq!(observed.provider_revision.as_deref(), Some("r1"));
    assert_eq!(
        server.requests("v1/recordings/detail").await[0].1["recording_id"],
        "item/1"
    );
    hito.identity
        .create_user(
            &alice,
            hito_core::Credentials {
                username: "bob".into(),
                password: "provider bob password".into(),
            },
        )
        .await
        .unwrap();
    let session = hito
        .identity
        .login(hito_core::Credentials {
            username: "bob".into(),
            password: "provider bob password".into(),
        })
        .await
        .unwrap();
    let bob = hito.identity.authenticate(&session.token).await.unwrap();
    assert!(
        hito.catalog
            .recordings(&bob, Default::default())
            .await
            .unwrap()
            .items
            .is_empty()
    );
    assert!(hito.catalog.sources(&bob, &adopted.id).await.is_err());
    assert!(
        hito.catalog
            .recording_metadata(&bob, &adopted.id, Default::default())
            .await
            .is_err()
    );
    assert!(
        hito.identity
            .set_library_access(&alice, &adopted.library_id, &bob.user().id, true)
            .await
            .is_err()
    );
    let bob_install = hito
        .plugins
        .install(
            &bob,
            InstallPlugin {
                accept_manifest_digest: None,
                service_id: installed.service_id,
                service_revision: 1,
                bearer_token: Some("bob-provider-token".into()),
            },
        )
        .await
        .unwrap();
    let bob_recording = hito
        .catalog
        .adopt_provider(
            &bob,
            &bob_install.id,
            AdoptProviderRecording {
                installation_revision: 1,
                provider_recording_id: "item/1".into(),
            },
        )
        .await
        .unwrap();
    assert_ne!(bob_recording.id, adopted.id);
    assert_ne!(bob_recording.library_id, adopted.library_id);
    assert_eq!(
        hito.catalog
            .recordings(&alice, Default::default())
            .await
            .unwrap()
            .total,
        1
    );
    assert!(
        hito.catalog
            .sources(&alice, &bob_recording.id)
            .await
            .is_err()
    );
    assert!(
        hito.identity
            .set_library_access(&alice, &bob_recording.library_id, &alice.user().id, true)
            .await
            .is_err()
    );
    assert!(
        hito.identity
            .set_library_access(&alice, &bob_recording.library_id, &bob.user().id, false)
            .await
            .is_err()
    );
}

#[tokio::test]
async fn provider_refresh_preserves_collection_and_personal_edits_and_removal_withdraws_audio() {
    let server = fixture::ProviderServer::start().await;
    let (_dir, hito, actor, _shared, _music) = support::library("provider-refresh").await;
    let service = hito
        .plugins
        .offer_service(
            &actor,
            OfferPluginService {
                name: "Fixture".into(),
                base_url: server.url.clone(),
            },
        )
        .await
        .unwrap();
    let installed = hito
        .plugins
        .install(
            &actor,
            InstallPlugin {
                accept_manifest_digest: None,
                service_id: service.id,
                service_revision: 1,
                bearer_token: None,
            },
        )
        .await
        .unwrap();
    server
        .reply(
            "v1/recordings/detail",
            serde_json::json!({"id":"item/1","title":"Original title","artist":"Provider artist"}),
        )
        .await;
    server
        .reply(
            "v1/sources",
            serde_json::json!({"items":[{"id":"audio/1","label":"Original","ready":true}]}),
        )
        .await;
    let request = || AdoptProviderRecording {
        installation_revision: 1,
        provider_recording_id: "item/1".into(),
    };
    let first = hito
        .catalog
        .adopt_provider(&actor, &installed.id, request())
        .await
        .unwrap();
    let playlist = uuid::Uuid::new_v4();
    let recording = uuid::Uuid::parse_str(&first.id).unwrap();
    support::synchronize(
        &hito,
        &actor,
        vec![
            hito_core::Action::PlaylistCreate {
                playlist_id: playlist,
                name: "Saved provider music".into(),
            },
            hito_core::Action::EntryInsert {
                playlist_id: playlist,
                entry_id: uuid::Uuid::new_v4(),
                recording_id: recording,
                position: 0,
            },
            hito_core::Action::LikeSet {
                recording_id: recording,
                liked: true,
            },
            hito_core::Action::RecordingTitleSet {
                recording_id: recording,
                title: "My title".into(),
            },
        ],
    )
    .await;
    server.reply("v1/recordings/detail", serde_json::json!({"id":"item/1","title":"Updated title","artist":"Provider artist","revision":"new"})).await;
    let refreshed = hito
        .catalog
        .adopt_provider(&actor, &installed.id, request())
        .await
        .unwrap();
    assert_eq!(refreshed.id, first.id);
    assert_eq!(refreshed.title, "My title");
    assert_eq!(refreshed.inherited_title, "Updated title");
    assert!(refreshed.liked);
    hito.plugins.remove(&actor, &installed.id, 1).await.unwrap();
    let playlist = hito
        .collection
        .playlist(&actor, &playlist.to_string())
        .await
        .unwrap();
    assert_eq!(playlist.entries[0].recording_id, Some(first.id.clone()));
    assert_eq!(playlist.entries[0].title, "My title");
    assert!(!playlist.entries[0].available);
    assert!(!hito.catalog.sources(&actor, &first.id).await.unwrap()[0].ready);
    assert_eq!(
        hito.catalog
            .recording_metadata(&actor, &first.id, Default::default())
            .await
            .unwrap()
            .observations
            .items
            .len(),
        2
    );
}
