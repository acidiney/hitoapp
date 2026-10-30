use hito_core::test_support::artist_identity_selection_catalog as catalog;
#[path = "../fixtures/artist_identity_selection_library.rs"]
mod library;
use hito_core::test_support::support;

use futures_util::FutureExt;
use hito_core::{
    ChooseRecordingArtistIdentity, Error, MetadataProvider, RecordingArtistIdentityState,
    ResetRecordingArtistIdentity, UpdateMetadataSettings,
};
use std::panic::{AssertUnwindSafe, resume_unwind};
use uuid::Uuid;

#[tokio::test]
async fn accepted_identity_reads_reset_and_receipt_replay_remain_local_after_disable_and_disconnect()
 {
    let mut library = library::Library::prepare().await;
    let result = AssertUnwindSafe(exercise(&library)).catch_unwind().await;
    library.origin.shutdown().await;
    library.catalog.finish().await;
    if let Err(panic) = result {
        resume_unwind(panic);
    }
}

async fn exercise(library: &library::Library) {
    let hito = &library.hito;
    let actor = &library.actor;
    let mut commands = Vec::new();
    let mut receipts = Vec::new();
    for recording in [&library.local, &library.imported] {
        let context = hito
            .artist_catalog
            .begin_artist_review(actor, recording, library.metadata_provider.clone())
            .await
            .unwrap();
        let association = hito
            .artist_catalog
            .confirm_artist_review(actor, &context, catalog::ALPHA)
            .await
            .unwrap();
        let request = ChooseRecordingArtistIdentity {
            request_id: Uuid::new_v4(),
            expected_revision: 0,
            association_id: Uuid::parse_str(&association.id).unwrap(),
            context,
        };
        let command = serde_json::to_value(&request).unwrap();
        receipts.push(
            hito.artist_catalog
                .choose_recording_artist_identity(actor, recording, request)
                .await
                .unwrap(),
        );
        commands.push(command);
    }
    let mut before = library.snapshot().await;
    before.as_object_mut().unwrap().remove("settings");
    before.as_object_mut().unwrap().remove("plugins");
    let requests = library.catalog.requests();
    let origin = library.origin_requests().await;
    let group_id = &receipts[0].selection.as_ref().unwrap().group_id;
    let settings = hito.artist_catalog.metadata_settings(actor).await.unwrap();
    hito.artist_catalog
        .set_metadata_settings(
            actor,
            UpdateMetadataSettings {
                revision: settings.revision,
                enabled: false,
                providers: settings.providers,
            },
        )
        .await
        .unwrap();
    let group = hito
        .artist_catalog
        .artist_identity_group(actor, group_id, Default::default())
        .await
        .unwrap();
    assert_eq!(group.recordings.total, 2);
    assert_eq!(group.releases.total, 2);
    let reset_imported = hito
        .artist_catalog
        .reset_recording_artist_identity(
            actor,
            &library.imported,
            ResetRecordingArtistIdentity {
                request_id: Uuid::new_v4(),
                expected_revision: 1,
            },
        )
        .await
        .unwrap();
    assert_eq!(reset_imported.revision, 2);
    let replay = hito
        .artist_catalog
        .choose_recording_artist_identity(
            actor,
            &library.imported,
            serde_json::from_value(commands[1].clone()).unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(
        serde_json::to_value(replay).unwrap(),
        serde_json::to_value(&receipts[1]).unwrap()
    );
    let mut new_command = commands[1].clone();
    new_command["request_id"] = serde_json::to_value(Uuid::new_v4()).unwrap();
    new_command["expected_revision"] = 2.into();
    assert!(
        matches!(
            hito.artist_catalog
                .choose_recording_artist_identity(
                    actor,
                    &library.imported,
                    serde_json::from_value(new_command).unwrap()
                )
                .await,
            Err(Error::Conflict(_))
        ),
        "Disabled metadata must reject new selection while preserving acknowledgement recovery"
    );
    let current = hito
        .artist_catalog
        .recording_artist_identity_choice(actor, &library.imported)
        .await
        .unwrap();
    assert!(
        current.revision == 2 && matches!(current.state, RecordingArtistIdentityState::Unselected)
    );
    let settings = hito.artist_catalog.metadata_settings(actor).await.unwrap();
    hito.artist_catalog
        .set_metadata_settings(
            actor,
            UpdateMetadataSettings {
                revision: settings.revision,
                enabled: true,
                providers: settings.providers,
            },
        )
        .await
        .unwrap();
    let MetadataProvider::HttpPlugin { installation_id } = &library.metadata_provider else {
        unreachable!()
    };
    let installed = hito
        .plugins
        .settings(actor)
        .await
        .unwrap()
        .installations
        .into_iter()
        .find(|item| &item.id == installation_id)
        .unwrap();
    hito.plugins
        .remove(actor, installation_id, installed.revision)
        .await
        .unwrap();
    let current = hito
        .artist_catalog
        .recording_artist_identity_choice(actor, &library.local)
        .await
        .unwrap();
    assert!(
        current.revision == 1 && matches!(current.state, RecordingArtistIdentityState::Selected(_))
    );
    let group = hito
        .artist_catalog
        .artist_identity_group(actor, group_id, Default::default())
        .await
        .unwrap();
    assert_eq!(group.recordings.total, 1);
    assert_eq!(group.recordings.items[0].recording.id, library.local);
    assert_eq!(group.group.provider, library.metadata_provider);
    assert_eq!(group.group.name.value, catalog::CREDIT);
    let reset_id = Uuid::new_v4();
    let reset = hito
        .artist_catalog
        .reset_recording_artist_identity(
            actor,
            &library.local,
            ResetRecordingArtistIdentity {
                request_id: reset_id,
                expected_revision: 1,
            },
        )
        .await
        .unwrap();
    let replay = hito
        .artist_catalog
        .choose_recording_artist_identity(
            actor,
            &library.local,
            serde_json::from_value(commands[0].clone()).unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(
        serde_json::to_value(replay).unwrap(),
        serde_json::to_value(&receipts[0]).unwrap()
    );
    let replay = hito
        .artist_catalog
        .reset_recording_artist_identity(
            actor,
            &library.local,
            ResetRecordingArtistIdentity {
                request_id: reset_id,
                expected_revision: 1,
            },
        )
        .await
        .unwrap();
    assert_eq!(
        serde_json::to_value(replay).unwrap(),
        serde_json::to_value(reset).unwrap()
    );
    let mut new_command = commands[0].clone();
    new_command["request_id"] = serde_json::to_value(Uuid::new_v4()).unwrap();
    new_command["expected_revision"] = 2.into();
    assert!(
        matches!(
            hito.artist_catalog
                .choose_recording_artist_identity(
                    actor,
                    &library.local,
                    serde_json::from_value(new_command).unwrap()
                )
                .await,
            Err(Error::NotFound)
        ),
        "A disconnected catalog must not authorize a new selection"
    );
    assert_eq!(
        hito.artist_catalog
            .artist_identity_groups(actor, Default::default())
            .await
            .unwrap()
            .total,
        0
    );
    let current = hito
        .artist_catalog
        .recording_artist_identity_choice(actor, &library.local)
        .await
        .unwrap();
    assert!(
        current.revision == 2 && matches!(current.state, RecordingArtistIdentityState::Unselected)
    );
    let settings = hito.artist_catalog.metadata_settings(actor).await.unwrap();
    assert!(settings.enabled && settings.providers == vec![library.metadata_provider.clone()]);
    assert!(
        !hito
            .plugins
            .settings(actor)
            .await
            .unwrap()
            .installations
            .iter()
            .any(|item| &item.id == installation_id)
    );
    let mut after = library.snapshot().await;
    after.as_object_mut().unwrap().remove("settings");
    after.as_object_mut().unwrap().remove("plugins");
    assert_eq!(after, before);
    assert_eq!(library.catalog.requests(), requests);
    assert_eq!(library.origin_requests().await, origin);
}
