use super::artist_catalog_credit_members::{Library, prepare};
use futures_util::FutureExt;
use hito_core::{
    Action, ChooseCreditMemberIdentity, CoreConfig, HostDeviceId, Operation,
    RecordingArtistIdentityState, SyncBatch,
    test_support::{
        artist_catalog_discography_service as catalog, database_owner::DatabaseOwner,
        hang_guard::hang_guard, playlist_service as origin,
    },
};
use serde_json::json;
use sqlx::{Connection, SqliteConnection, sqlite::SqliteConnectOptions};
use std::{panic::AssertUnwindSafe, path::Path, time::Duration};

type MemberChoiceRow = (
    String,
    String,
    i64,
    Option<String>,
    Option<String>,
    Option<String>,
    Option<String>,
    i64,
);
type WholeChoiceRow = (
    i64,
    Option<String>,
    Option<String>,
    Option<String>,
    Option<String>,
    Option<String>,
    i64,
);
type PersonalMetadataRow = (
    Option<String>,
    Option<String>,
    String,
    String,
    Option<String>,
    Option<String>,
);

struct OwnedRecordingSpec<'a> {
    playlist_id: &'a str,
    snapshot: &'a str,
    occurrence: &'a str,
    recording_id: &'a str,
    title: &'a str,
    artist: &'a str,
    artists: serde_json::Value,
}

#[tokio::test]
async fn startup_repairs_a_missing_main_member_after_reopen_and_is_idempotent() {
    let mut library =
        Some(hang_guard("the owned library must open", Box::pin(Library::open())).await);
    let mut origin = Some(
        hang_guard(
            "the owned playlist provider must start",
            Box::pin(origin::ProviderServer::start()),
        )
        .await,
    );
    let mut catalog = Some(
        hang_guard(
            "the owned artist catalog must start",
            Box::pin(catalog::CatalogServer::start()),
        )
        .await,
    );
    catalog.as_mut().unwrap().enable_discography();
    let directory = library.as_ref().unwrap().directory.clone();
    let token = library.as_ref().unwrap().token.clone();
    let mut recording = String::new();
    let mut main = String::new();
    let mut singleton_recording = String::new();
    let mut singleton_member = String::new();
    let mut registration_recording = String::new();
    let mut registration_main = String::new();
    let mut registration_guest = String::new();
    let mut registration_anchor_recording = String::new();
    let mut registration_anchor = String::new();
    let mut override_recording = String::new();
    let mut override_guest = String::new();
    let mut override_request_id = String::new();
    let mut reopened_services = None;
    let mut reopened_owner = None;
    let outcome = AssertUnwindSafe(Box::pin(async {
        let prepared = hang_guard(
            "the structured credit must be prepared",
            Box::pin(prepare(
                library.as_ref().unwrap(),
                origin.as_ref().unwrap(),
                catalog.as_ref().unwrap(),
            )),
        )
        .await;
        recording = prepared.recording.clone();
        main = prepared.ada.clone();
        (singleton_recording, singleton_member) = import_singleton(
            library.as_ref().unwrap(), origin.as_ref().unwrap(), &prepared.playlist_installation,
        ).await;
        (override_recording, override_guest) = import_collaboration(
            library.as_ref().unwrap(), origin.as_ref().unwrap(), &prepared.playlist_installation,
        ).await;
        (
            registration_anchor_recording,
            registration_anchor,
            registration_recording,
            registration_main,
            registration_guest,
        ) = import_registration_case(
            library.as_ref().unwrap(),
            origin.as_ref().unwrap(),
            &prepared.playlist_installation,
        )
        .await;
        let singleton_before = library.as_ref().unwrap().hito.artist_catalog
            .recording_credit_member_identity_choice(
                &library.as_ref().unwrap().actor, &singleton_recording, &singleton_member,
            ).await.unwrap();
        assert_eq!(singleton_before.revision, 0);
        assert!(matches!(singleton_before.state, RecordingArtistIdentityState::Unselected));
        let registration_anchor_before = library.as_ref().unwrap().hito.artist_catalog
            .recording_credit_member_identity_choice(
                &library.as_ref().unwrap().actor, &registration_anchor_recording, &registration_anchor,
            ).await.unwrap();
        assert_eq!(registration_anchor_before.revision, 0);
        assert!(matches!(registration_anchor_before.state, RecordingArtistIdentityState::Unselected));
        let registration_main_before = library.as_ref().unwrap().hito.artist_catalog
            .recording_credit_member_identity_choice(
                &library.as_ref().unwrap().actor, &registration_recording, &registration_main,
            ).await.unwrap();
        assert_eq!(registration_main_before.revision, 0);
        assert_eq!(registration_main_before.role, hito_plugin_sdk::CreditRole::Main);
        assert!(matches!(registration_main_before.state, RecordingArtistIdentityState::Unselected));
        let registration_guest_before = library.as_ref().unwrap().hito.artist_catalog
            .recording_credit_member_identity_choice(
                &library.as_ref().unwrap().actor, &registration_recording, &registration_guest,
            ).await.unwrap();
        assert_eq!(registration_guest_before.revision, 0);
        assert_eq!(registration_guest_before.role, hito_plugin_sdk::CreditRole::Featured);
        assert!(matches!(registration_guest_before.state, RecordingArtistIdentityState::Unselected));
        let override_context = library.as_ref().unwrap().hito.artist_catalog
            .begin_credit_member_review(&library.as_ref().unwrap().actor,
                &override_recording, &override_guest, prepared.provider.clone()).await.unwrap();
        let override_association = library.as_ref().unwrap().hito.artist_catalog
            .confirm_credit_member_review(&library.as_ref().unwrap().actor,
                &override_context, catalog::ARTIST_ID).await.unwrap();
        let request_id = uuid::Uuid::new_v4();
        override_request_id = request_id.to_string();
        library.as_ref().unwrap().hito.artist_catalog
            .choose_credit_member_identity(&library.as_ref().unwrap().actor,
                &override_recording, &override_guest, ChooseCreditMemberIdentity {
                    request_id, expected_revision: 0,
                    association_id: uuid::Uuid::parse_str(&override_association.id).unwrap(),
                    context: override_context,
                }).await.unwrap();
        library.as_ref().unwrap().hito.collection.apply(
            &library.as_ref().unwrap().actor,
            SyncBatch { device_id: "member139-repair".into(), operations: vec![Operation {
                id: uuid::Uuid::new_v4(), action: Action::RecordingArtistSet {
                    recording_id: uuid::Uuid::parse_str(&override_recording).unwrap(),
                    artist: "Private artist override".into(),
                },
            }] },
        ).await.unwrap();
        let first = library
            .as_ref()
            .unwrap()
            .hito
            .artist_catalog
            .recording_credit_member_identity_choice(
                &library.as_ref().unwrap().actor,
                &recording,
                &main,
            )
            .await
            .unwrap();
        assert_eq!(first.revision, 0);
        assert!(matches!(
            first.state,
            RecordingArtistIdentityState::Unselected
        ));
        let guest_association = super::artist_catalog_credit_members::exercise(
            library.as_ref().unwrap(),
            &prepared,
            catalog.as_ref().unwrap(),
        )
        .await;
        library
            .as_ref()
            .unwrap()
            .hito
            .collection
            .apply(
                &library.as_ref().unwrap().actor,
                SyncBatch {
                    device_id: "member139-repair".into(),
                    operations: vec![Operation {
                        id: uuid::Uuid::new_v4(),
                        action: Action::RecordingTitleSet {
                            recording_id: uuid::Uuid::parse_str(&recording).unwrap(),
                            title: "Owned private repair title".into(),
                        },
                    }],
                },
            )
            .await
            .unwrap();
        let member_context = library
            .as_ref()
            .unwrap()
            .hito
            .artist_catalog
            .begin_credit_member_review(
                &library.as_ref().unwrap().actor,
                &recording,
                &prepared.cy,
                prepared.provider.clone(),
            )
            .await
            .unwrap();
        let guest_request_id = uuid::Uuid::new_v4();
        let guest_receipt = library
            .as_ref()
            .unwrap()
            .hito
            .artist_catalog
            .choose_credit_member_identity(
                &library.as_ref().unwrap().actor,
                &recording,
                &prepared.cy,
                ChooseCreditMemberIdentity {
                    request_id: guest_request_id,
                    expected_revision: 0,
                    association_id: uuid::Uuid::parse_str(&guest_association).unwrap(),
                    context: member_context,
                },
            )
            .await
            .unwrap();
        assert_eq!(guest_receipt.revision, 1);
        let database = directory.path().join("hito.db");
        let singleton_credit_state = credit_identity_state(&database, &singleton_recording).await;
        assert_eq!(singleton_credit_state["whole_artist_id"], singleton_member);
        assert_eq!(singleton_credit_state["key_artist_id"], singleton_member);
        let registration_credit_state = credit_identity_state(&database, &registration_recording).await;
        assert_eq!(registration_credit_state["whole_artist_id"], registration_anchor);
        assert_eq!(registration_credit_state["key_artist_id"], registration_anchor);
        assert_ne!(registration_credit_state["whole_artist_id"], registration_main);
        let before_registration_membership =
            artist_membership_state(&database, &registration_recording, &registration_main, "owned registration anchor").await;
        assert_eq!(before_registration_membership, (1, 1));
        let before_private_state =
            private_state(&database, &recording, &guest_request_id.to_string()).await;
        assert_eq!(
            before_private_state["personal"][0],
            "Owned private repair title"
        );
        assert!(before_private_state["personal"][1].is_null());
        let before_override_state = private_state(
            &database, &override_recording, &override_request_id,
        ).await;
        assert_eq!(before_override_state["personal"][1], "Private artist override");
        assert_eq!(before_override_state["personal"][3], "Private artist override");
        assert!(before_override_state["personal"][5].is_string());
        assert_eq!(before_override_state["member_count"], 0);

        let music = directory.path().join("music");
        library.take().unwrap().close().await;
        let mut connection = SqliteConnection::connect_with(
            &SqliteConnectOptions::new()
                .filename(&database)
                .create_if_missing(false)
                .foreign_keys(true),
        )
        .await
        .unwrap();
        let removed = sqlx::query(
            "DELETE FROM account_recording_credit_members WHERE recording_id IN (?,?,?) AND role='main'",
        )
        .bind(&recording)
        .bind(&singleton_recording)
        .bind(&registration_recording)
        .execute(&mut connection)
        .await
        .unwrap()
        .rows_affected();
        assert_eq!(
            removed, 3,
            "the owned fixture must remove only its historical main projection"
        );
        let unused_main_artist = sqlx::query(
            "DELETE FROM account_artists
             WHERE id=? AND name='owned registration anchor' COLLATE BINARY
                AND NOT EXISTS (SELECT 1 FROM account_recording_credits reference
                    WHERE reference.user_id=account_artists.user_id AND reference.artist_id=account_artists.id)
                AND NOT EXISTS (SELECT 1 FROM account_credit_keys reference
                    WHERE reference.user_id=account_artists.user_id AND reference.artist_id=account_artists.id)
                AND NOT EXISTS (SELECT 1 FROM account_recording_credit_members reference
                    WHERE reference.user_id=account_artists.user_id AND reference.artist_id=account_artists.id)
                AND NOT EXISTS (SELECT 1 FROM account_source_credits reference
                    WHERE reference.user_id=account_artists.user_id AND reference.artist_id=account_artists.id)
                AND NOT EXISTS (SELECT 1 FROM account_releases reference
                    WHERE reference.user_id=account_artists.user_id AND reference.artist_id=account_artists.id)
                AND NOT EXISTS (SELECT 1 FROM recording_artist_associations reference
                    WHERE reference.user_id=account_artists.user_id AND reference.artist_id=account_artists.id)
                AND NOT EXISTS (SELECT 1 FROM recording_artist_identity_choices reference
                    WHERE reference.user_id=account_artists.user_id AND reference.artist_id=account_artists.id)
                AND NOT EXISTS (SELECT 1 FROM recording_artist_member_identity_choices reference
                    WHERE reference.user_id=account_artists.user_id AND reference.artist_id=account_artists.id)",
        )
        .bind(&registration_main)
        .execute(&mut connection)
        .await
        .unwrap()
        .rows_affected();
        assert_eq!(unused_main_artist, 1,
            "the historical main artist used for registration repair must have no external references");
        let missing_registration_artist_rows: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM account_artists WHERE id=? AND name='owned registration anchor' COLLATE BINARY",
        ).bind(&registration_main).fetch_one(&mut connection).await.unwrap();
        assert_eq!(missing_registration_artist_rows, 0);
        let singleton_artist_rows: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM account_artists WHERE id=? AND name='Owned singleton main' COLLATE BINARY",
        ).bind(&singleton_member).fetch_one(&mut connection).await.unwrap();
        assert_eq!(singleton_artist_rows, 1,
            "the singleton main artist remains because the whole credit references it");
        let retained_featured_rows: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM account_recording_credit_members WHERE user_id=(SELECT user_id FROM account_recording_credits WHERE recording_id=?) AND recording_id=? AND artist_id=? AND role='featured'",
        ).bind(&registration_recording).bind(&registration_recording).bind(&registration_guest)
            .fetch_one(&mut connection).await.unwrap();
        assert_eq!(retained_featured_rows, 1);
        connection.close().await.unwrap();
        let before_singleton_credit_state = singleton_credit_state;
        let before_registration_credit_state = registration_credit_state;

        let owner = DatabaseOwner::retain(directory.clone());
        let mut config = CoreConfig::new(
            database.clone(),
            music.clone(),
            HostDeviceId::new("artist-credit-members").unwrap(),
        );
        owner.attach(&mut config);
        reopened_owner = Some(owner);
        reopened_services = Some(hito_core::open(config).await.unwrap());
        let actor = reopened_services
            .as_ref()
            .unwrap()
            .identity
            .authenticate(&token)
            .await
            .unwrap();
        let repaired = reopened_services
            .as_ref()
            .unwrap()
            .artist_catalog
            .recording_credit_member_identity_choice(&actor, &recording, &main)
            .await
            .unwrap();
        assert_eq!(repaired.artist_id, main);
        assert_eq!(repaired.revision, 1);
        assert!(matches!(
            repaired.state,
            RecordingArtistIdentityState::Selected(_)
        ));
        assert_eq!(
            private_state(&database, &recording, &guest_request_id.to_string()).await,
            before_private_state,
            "repair must retain the featured choice, whole choice, and immutable receipt"
        );
        assert_eq!(
            private_state(&database, &override_recording, &override_request_id).await,
            before_override_state,
            "repair must preserve private artist override, retired member choice and receipt"
        );
        let singleton_metadata = reopened_services.as_ref().unwrap().catalog
            .recording_metadata(&actor, &singleton_recording, Default::default()).await.unwrap();
        let singleton_public_member = singleton_metadata.credited_artists.iter()
            .find(|member| member.name == "Owned singleton main").unwrap();
        assert_eq!(singleton_public_member.role, hito_plugin_sdk::CreditRole::Main);
        let singleton_current_id = singleton_public_member.artist_id.clone().unwrap();
        assert_eq!(singleton_current_id, singleton_member);
        let singleton_repaired = reopened_services.as_ref().unwrap().artist_catalog
            .recording_credit_member_identity_choice(&actor, &singleton_recording, &singleton_current_id)
            .await.unwrap();
        assert_eq!(singleton_repaired.artist_id, singleton_current_id);
        assert_eq!(singleton_repaired.revision, 0);
        assert_eq!(singleton_repaired.role, hito_plugin_sdk::CreditRole::Main);
        assert!(matches!(singleton_repaired.state, RecordingArtistIdentityState::Unselected));
        let singleton_rows = artist_membership_state(
            &database, &singleton_recording, &singleton_repaired.artist_id, "Owned singleton main",
        ).await;
        assert_eq!(singleton_rows, (1, 1));
        assert_eq!(
            credit_identity_state(&database, &singleton_recording).await,
            before_singleton_credit_state,
            "singleton whole-credit and key IDs remain unchanged"
        );
        let registration_metadata = reopened_services.as_ref().unwrap().catalog
            .recording_metadata(&actor, &registration_recording, Default::default()).await.unwrap();
        let registration_public_main = registration_metadata.credited_artists.iter()
            .find(|member| member.name == "owned registration anchor").unwrap();
        assert_eq!(registration_public_main.role, hito_plugin_sdk::CreditRole::Main);
        let repaired_registration_main = registration_public_main.artist_id.clone().unwrap();
        let registration_repaired = reopened_services.as_ref().unwrap().artist_catalog
            .recording_credit_member_identity_choice(&actor, &registration_recording, &repaired_registration_main)
            .await.unwrap();
        assert_eq!(registration_repaired.artist_id, repaired_registration_main);
        assert_eq!(registration_repaired.revision, 0);
        assert_eq!(registration_repaired.role, hito_plugin_sdk::CreditRole::Main);
        assert!(matches!(registration_repaired.state, RecordingArtistIdentityState::Unselected));
        assert_eq!(
            artist_membership_state(&database, &registration_recording, &repaired_registration_main, "owned registration anchor").await,
            (1, 1),
            "startup must recreate one named artist and one main membership"
        );
        assert_eq!(
            artist_membership_state(&database, &registration_recording, &registration_guest, "Owned registration guest").await,
            (1, 1),
            "the featured membership survives main artist registration repair"
        );
        let registration_guest_repaired = reopened_services.as_ref().unwrap().artist_catalog
            .recording_credit_member_identity_choice(&actor, &registration_recording, &registration_guest)
            .await.unwrap();
        assert_eq!(registration_guest_repaired.revision, 0);
        assert_eq!(registration_guest_repaired.role, hito_plugin_sdk::CreditRole::Featured);
        assert!(matches!(registration_guest_repaired.state, RecordingArtistIdentityState::Unselected));
        assert_eq!(
            credit_identity_state(&database, &registration_recording).await,
            before_registration_credit_state,
            "registration repair retains whole-credit and key identities"
        );
        let services = reopened_services.take().unwrap();
        services.playback.close().await.unwrap();
        services.replicas.close().await.unwrap();
        services.catalog_replication.close().await.unwrap();
        drop(services);
        let owner = reopened_owner.take().unwrap();
        let retired = owner.retire(Duration::from_secs(5)).await;
        assert!(retired.resources_retired && !retired.deadline_exceeded);
        assert_eq!(owner.observe().active_resources(), 0);

        let owner = DatabaseOwner::retain(directory.clone());
        let mut config = CoreConfig::new(
            database.clone(),
            music.clone(),
            HostDeviceId::new("artist-credit-members").unwrap(),
        );
        owner.attach(&mut config);
        reopened_owner = Some(owner);
        reopened_services = Some(hito_core::open(config).await.unwrap());
        let actor = reopened_services
            .as_ref()
            .unwrap()
            .identity
            .authenticate(&token)
            .await
            .unwrap();
        let repeated = reopened_services
            .as_ref()
            .unwrap()
            .artist_catalog
            .recording_credit_member_identity_choice(&actor, &recording, &main)
            .await
            .unwrap();
        assert_eq!(repeated.artist_id, main);
        assert_eq!(repeated.revision, 1);
        assert!(matches!(
            repeated.state,
            RecordingArtistIdentityState::Selected(_)
        ));
        assert_eq!(
            private_state(&database, &recording, &guest_request_id.to_string()).await,
            before_private_state
        );
        assert_eq!(private_state(&database, &override_recording, &override_request_id).await,
            before_override_state);
        let singleton_repeated = reopened_services.as_ref().unwrap().artist_catalog
            .recording_credit_member_identity_choice(&actor, &singleton_recording, &singleton_repaired.artist_id)
            .await.unwrap();
        assert_eq!(singleton_repeated.artist_id, singleton_repaired.artist_id);
        assert_eq!(singleton_repeated.revision, 0);
        assert_eq!(singleton_repeated.role, hito_plugin_sdk::CreditRole::Main);
        assert!(matches!(singleton_repeated.state, RecordingArtistIdentityState::Unselected));
        assert_eq!(artist_membership_state(
            &database, &singleton_recording, &singleton_repaired.artist_id, "Owned singleton main",
        ).await, (1, 1));
        let registration_metadata = reopened_services.as_ref().unwrap().catalog
            .recording_metadata(&actor, &registration_recording, Default::default()).await.unwrap();
        let registration_public_main = registration_metadata.credited_artists.iter()
            .find(|member| member.name == "owned registration anchor").unwrap();
        assert_eq!(registration_public_main.role, hito_plugin_sdk::CreditRole::Main);
        let registration_current_id = registration_public_main.artist_id.as_deref().unwrap();
        assert_eq!(registration_current_id, repaired_registration_main);
        let registration_repeated = reopened_services.as_ref().unwrap().artist_catalog
            .recording_credit_member_identity_choice(&actor, &registration_recording, registration_current_id)
            .await.unwrap();
        assert_eq!(registration_repeated.artist_id, repaired_registration_main);
        assert_eq!(registration_repeated.revision, 0);
        assert_eq!(registration_repeated.role, hito_plugin_sdk::CreditRole::Main);
        assert!(matches!(registration_repeated.state, RecordingArtistIdentityState::Unselected));
        assert_eq!(artist_membership_state(
            &database, &registration_recording, registration_current_id, "owned registration anchor",
        ).await, (1, 1));
        assert_eq!(artist_membership_state(
            &database, &registration_recording, &registration_guest, "Owned registration guest",
        ).await, (1, 1));
        assert_eq!(credit_identity_state(&database, &registration_recording).await, before_registration_credit_state);
        let services = reopened_services.take().unwrap();
        services.playback.close().await.unwrap();
        services.replicas.close().await.unwrap();
        services.catalog_replication.close().await.unwrap();
        drop(services);
        let owner = reopened_owner.take().unwrap();
        let retired = owner.retire(Duration::from_secs(5)).await;
        assert!(retired.resources_retired && !retired.deadline_exceeded);
        assert_eq!(owner.observe().active_resources(), 0);
    }))
    .catch_unwind()
    .await;
    if let Some(library) = library.take() {
        library.close().await;
    }
    if let Some(services) = reopened_services.take() {
        let _ = services.playback.close().await;
        let _ = services.replicas.close().await;
        let _ = services.catalog_replication.close().await;
        drop(services);
    }
    if let Some(owner) = reopened_owner.take() {
        let _ = owner.retire(Duration::from_secs(5)).await;
    }
    let catalog_joined = if let Some(catalog) = catalog.as_mut() {
        catalog.finish().await
    } else {
        true
    };
    let origin_joined = if let Some(origin) = origin.take() {
        origin.shutdown().await;
        true
    } else {
        true
    };
    assert!(catalog_joined && origin_joined);
    if let Err(panic) = outcome {
        std::panic::resume_unwind(panic);
    }
}

async fn import_singleton(
    library: &Library,
    origin: &origin::ProviderServer,
    installation: &hito_core::PluginInstallation,
) -> (String, String) {
    origin::playlist(
        origin,
        "owned-singleton-credit",
        "owned-singleton-credit-v1",
        vec![json!({
            "occurrence_id": "owned-singleton-appearance",
            "item": {"kind": "recording", "recording": {
                "id": "owned-singleton-recording", "title": "Owned singleton",
                "artist": "Owned singleton collection", "duration_ms": 100_000,
                "artists": [{"name": "Owned singleton main", "role": "main"}]
            }}
        })],
    )
    .await;
    let imported = library
        .hito
        .collection
        .import_provider_playlist(
            &library.actor,
            &installation.id,
            hito_core::ImportProviderPlaylist {
                request_id: uuid::Uuid::new_v4(),
                installation_revision: installation.revision,
                playlist_reference: "https://provider.invalid/owned-singleton-credit".into(),
            },
        )
        .await
        .unwrap();
    let playlist = library
        .hito
        .collection
        .playlist(&library.actor, &imported.playlist_id.to_string())
        .await
        .unwrap();
    let recording = playlist.entries[0]
        .recording_id
        .as_deref()
        .unwrap()
        .to_owned();
    let metadata = library
        .hito
        .catalog
        .recording_metadata(&library.actor, &recording, Default::default())
        .await
        .unwrap();
    let member = metadata
        .credited_artists
        .iter()
        .find(|member| {
            member.name == "Owned singleton main"
                && member.role == hito_plugin_sdk::CreditRole::Main
        })
        .unwrap()
        .artist_id
        .clone()
        .unwrap();
    (recording, member)
}

async fn import_registration_case(
    library: &Library,
    origin: &origin::ProviderServer,
    installation: &hito_core::PluginInstallation,
) -> (String, String, String, String, String) {
    let anchor_recording = import_owned_recording(
        library,
        origin,
        installation,
        OwnedRecordingSpec {
            playlist_id: "owned-registration-anchor",
            snapshot: "owned-registration-anchor-v1",
            occurrence: "owned-registration-anchor-appearance",
            recording_id: "owned-registration-anchor-recording",
            title: "Owned registration anchor",
            artist: "Owned registration anchor",
            artists: json!([{"name": "Owned registration anchor", "role": "main"}]),
        },
    )
    .await;
    let anchor_metadata = library
        .hito
        .catalog
        .recording_metadata(&library.actor, &anchor_recording, Default::default())
        .await
        .unwrap();
    let anchor_id = anchor_metadata
        .credited_artists
        .iter()
        .find(|member| member.name == "Owned registration anchor")
        .unwrap()
        .artist_id
        .clone()
        .unwrap();
    let registration_recording = import_owned_recording(
        library,
        origin,
        installation,
        OwnedRecordingSpec {
            playlist_id: "owned-registration-member",
            snapshot: "owned-registration-member-v1",
            occurrence: "owned-registration-member-appearance",
            recording_id: "owned-registration-member-recording",
            title: "Owned distinct whole credit",
            artist: "Owned registration anchor feat. Owned registration guest",
            artists: json!([
                {"name": "owned registration anchor", "role": "main"},
                {"name": "Owned registration guest", "role": "featured"}
            ]),
        },
    )
    .await;
    let registration_metadata = library
        .hito
        .catalog
        .recording_metadata(&library.actor, &registration_recording, Default::default())
        .await
        .unwrap();
    let registration_main = registration_metadata
        .credited_artists
        .iter()
        .find(|member| member.name == "owned registration anchor")
        .unwrap()
        .artist_id
        .clone()
        .unwrap();
    let registration_guest = registration_metadata
        .credited_artists
        .iter()
        .find(|member| member.name == "Owned registration guest")
        .unwrap()
        .artist_id
        .clone()
        .unwrap();
    assert_ne!(anchor_id, registration_main);
    (
        anchor_recording,
        anchor_id,
        registration_recording,
        registration_main,
        registration_guest,
    )
}

async fn import_owned_recording(
    library: &Library,
    origin: &origin::ProviderServer,
    installation: &hito_core::PluginInstallation,
    spec: OwnedRecordingSpec<'_>,
) -> String {
    origin::playlist(
        origin,
        spec.playlist_id,
        spec.snapshot,
        vec![json!({
            "occurrence_id": spec.occurrence,
            "item": {"kind": "recording", "recording": {
                "id": spec.recording_id, "title": spec.title, "artist": spec.artist,
                "duration_ms": 100_000, "artists": spec.artists
            }}
        })],
    )
    .await;
    let imported = library
        .hito
        .collection
        .import_provider_playlist(
            &library.actor,
            &installation.id,
            hito_core::ImportProviderPlaylist {
                request_id: uuid::Uuid::new_v4(),
                installation_revision: installation.revision,
                playlist_reference: format!("https://provider.invalid/{}", spec.playlist_id),
            },
        )
        .await
        .unwrap();
    let playlist = library
        .hito
        .collection
        .playlist(&library.actor, &imported.playlist_id.to_string())
        .await
        .unwrap();
    playlist.entries[0]
        .recording_id
        .as_deref()
        .unwrap()
        .to_owned()
}

async fn import_collaboration(
    library: &Library,
    origin: &origin::ProviderServer,
    installation: &hito_core::PluginInstallation,
) -> (String, String) {
    origin::playlist(
        origin,
        "owned-override-credit",
        "owned-override-credit-v1",
        vec![json!({
            "occurrence_id": "owned-override-appearance",
            "item": {"kind": "recording", "recording": {
                "id": "owned-override-recording", "title": "Owned override duet",
                "artist": "Ada feat. Cy", "duration_ms": 100_000,
                "artists": [
                    {"name": "Ada", "role": "main", "join": " feat. "},
                    {"name": "Cy", "role": "featured"}
                ]
            }}
        })],
    )
    .await;
    let imported = library
        .hito
        .collection
        .import_provider_playlist(
            &library.actor,
            &installation.id,
            hito_core::ImportProviderPlaylist {
                request_id: uuid::Uuid::new_v4(),
                installation_revision: installation.revision,
                playlist_reference: "https://provider.invalid/owned-override-credit".into(),
            },
        )
        .await
        .unwrap();
    let playlist = library
        .hito
        .collection
        .playlist(&library.actor, &imported.playlist_id.to_string())
        .await
        .unwrap();
    let recording = playlist.entries[0]
        .recording_id
        .as_deref()
        .unwrap()
        .to_owned();
    let metadata = library
        .hito
        .catalog
        .recording_metadata(&library.actor, &recording, Default::default())
        .await
        .unwrap();
    let guest = metadata
        .credited_artists
        .iter()
        .find(|member| member.name == "Cy" && member.role == hito_plugin_sdk::CreditRole::Featured)
        .unwrap()
        .artist_id
        .clone()
        .unwrap();
    (recording, guest)
}

async fn private_state(database: &Path, recording: &str, request_id: &str) -> serde_json::Value {
    let mut connection = SqliteConnection::connect_with(
        &SqliteConnectOptions::new()
            .filename(database)
            .create_if_missing(false)
            .foreign_keys(true),
    )
    .await
    .unwrap();
    let members: Vec<MemberChoiceRow> =
        sqlx::query_as("SELECT artist_id,role,revision,association_id,group_id,artist_operation_id,observation_id,retired
            FROM recording_artist_member_identity_choices WHERE recording_id=? ORDER BY artist_id")
            .bind(recording).fetch_all(&mut connection).await.unwrap();
    let whole: Vec<WholeChoiceRow> =
        sqlx::query_as("SELECT revision,association_id,group_id,artist_id,artist_operation_id,observation_id,retired
            FROM recording_artist_identity_choices WHERE recording_id=? ORDER BY user_id")
            .bind(recording).fetch_all(&mut connection).await.unwrap();
    let receipt: Option<(String, String)> = sqlx::query_as(
        "SELECT request_digest,receipt FROM artist_identity_choice_receipts WHERE request_id=?",
    )
    .bind(request_id)
    .fetch_optional(&mut connection)
    .await
    .unwrap();
    let personal: Option<PersonalMetadataRow> = sqlx::query_as(
        "SELECT override_title,override_artist,title,artist,title_operation_id,artist_operation_id
             FROM personal_recording_metadata WHERE recording_id=? ORDER BY user_id LIMIT 1",
    )
    .bind(recording)
    .fetch_optional(&mut connection)
    .await
    .unwrap();
    let member_count: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM account_recording_credit_members WHERE recording_id=?",
    )
    .bind(recording)
    .fetch_one(&mut connection)
    .await
    .unwrap();
    connection.close().await.unwrap();
    serde_json::json!({"member_choices":members,"whole_choices":whole,"receipt":receipt,"personal":personal,"member_count":member_count})
}

async fn artist_membership_state(
    database: &Path,
    recording: &str,
    artist: &str,
    name: &str,
) -> (i64, i64) {
    let mut connection = SqliteConnection::connect_with(
        &SqliteConnectOptions::new()
            .filename(database)
            .create_if_missing(false)
            .foreign_keys(true),
    )
    .await
    .unwrap();
    let artists: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM account_artists WHERE id=? AND name=? COLLATE BINARY",
    )
    .bind(artist)
    .bind(name)
    .fetch_one(&mut connection)
    .await
    .unwrap();
    let memberships: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM account_recording_credit_members WHERE recording_id=? AND artist_id=?",
    )
    .bind(recording)
    .bind(artist)
    .fetch_one(&mut connection)
    .await
    .unwrap();
    connection.close().await.unwrap();
    (artists, memberships)
}

async fn credit_identity_state(database: &Path, recording: &str) -> serde_json::Value {
    let mut connection = SqliteConnection::connect_with(
        &SqliteConnectOptions::new()
            .filename(database)
            .create_if_missing(false)
            .foreign_keys(true),
    )
    .await
    .unwrap();
    let row: (String, String, String) = sqlx::query_as(
        "SELECT credit.artist_id,key.artist_id,key.credit_key
         FROM account_recording_credits credit
         JOIN recordings recording ON recording.id=credit.recording_id
         JOIN recording_credit_observations observation
            ON observation.recording_id=recording.id AND observation.observation_id=recording.metadata_observation_id
         JOIN account_credit_keys key
            ON key.user_id=credit.user_id AND key.library_id=recording.library_id
                AND key.credit_key=observation.credit_key
         WHERE credit.recording_id=?",
    )
    .bind(recording)
    .fetch_one(&mut connection)
    .await
    .unwrap();
    connection.close().await.unwrap();
    serde_json::json!({
        "whole_artist_id": row.0,
        "key_artist_id": row.1,
        "credit_key": row.2,
    })
}
