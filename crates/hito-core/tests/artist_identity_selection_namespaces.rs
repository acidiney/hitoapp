use hito_core::test_support::artist_identity_selection_catalog as catalog;
#[path = "fixtures/artist_identity_selection_library.rs"]
mod library;
use hito_core::test_support::support;

use futures_util::FutureExt;
use hito_core::{
    ChooseRecordingArtistIdentity, InstallPlugin, MetadataProvider, OfferPluginService,
    UpdateMetadataSettings,
};
use std::panic::{AssertUnwindSafe, resume_unwind};
use uuid::Uuid;

#[tokio::test]
async fn equal_artist_ids_and_labels_from_different_personal_installations_remain_distinct() {
    let mut library = library::Library::prepare().await;
    let mut other = catalog::CatalogServer::start().await;
    let result = AssertUnwindSafe(exercise(&library, &other))
        .catch_unwind()
        .await;
    library.origin.shutdown().await;
    library.catalog.finish().await;
    other.finish().await;
    if let Err(panic) = result {
        resume_unwind(panic);
    }
}

async fn exercise(library: &library::Library, other: &catalog::CatalogServer) {
    let hito = &library.hito;
    let actor = &library.actor;
    let service = hito
        .plugins
        .offer_service(
            actor,
            OfferPluginService {
                name: "Other owned artist catalog".into(),
                base_url: other.url.clone(),
            },
        )
        .await
        .unwrap();
    let installed = hito
        .plugins
        .install(
            actor,
            InstallPlugin {
                service_id: service.id,
                service_revision: service.revision,
                bearer_token: None,
                accept_manifest_digest: None,
            },
        )
        .await
        .unwrap();
    let provider = MetadataProvider::HttpPlugin {
        installation_id: installed.id,
    };
    let settings = hito.catalog.metadata_settings(actor).await.unwrap();
    hito.catalog
        .set_metadata_settings(
            actor,
            UpdateMetadataSettings {
                revision: settings.revision,
                enabled: true,
                providers: vec![library.metadata_provider.clone(), provider.clone()],
            },
        )
        .await
        .unwrap();
    let before = library.snapshot().await;
    let requests = library.catalog.requests();
    let other_requests = other.requests();
    let origin = library.origin_requests().await;
    let mut selected = Vec::new();
    for (recording, provider) in [
        (&library.local, &library.metadata_provider),
        (&library.imported, &provider),
    ] {
        let context = hito
            .catalog
            .begin_artist_review(actor, recording, provider.clone())
            .await
            .unwrap();
        let association = hito
            .catalog
            .confirm_artist_review(actor, &context, catalog::ALPHA)
            .await
            .unwrap();
        selected.push(
            hito.catalog
                .choose_recording_artist_identity(
                    actor,
                    recording,
                    ChooseRecordingArtistIdentity {
                        request_id: Uuid::new_v4(),
                        expected_revision: 0,
                        association_id: Uuid::parse_str(&association.id).unwrap(),
                        context,
                    },
                )
                .await
                .unwrap(),
        );
    }
    assert_ne!(
        selected[0].selection.as_ref().unwrap().group_id,
        selected[1].selection.as_ref().unwrap().group_id
    );
    let groups = hito
        .catalog
        .artist_identity_groups(actor, Default::default())
        .await
        .unwrap();
    assert_eq!(groups.total, 2);
    assert!(
        groups
            .items
            .iter()
            .all(|group| group.provider_artist_id == catalog::ALPHA
                && group.name.value == catalog::CREDIT
                && group.recording_count == 1)
    );
    assert_ne!(groups.items[0].provider, groups.items[1].provider);
    for (receipt, recording) in selected.iter().zip([&library.local, &library.imported]) {
        let group = hito
            .catalog
            .artist_identity_group(
                actor,
                &receipt.selection.as_ref().unwrap().group_id,
                Default::default(),
            )
            .await
            .unwrap();
        assert_eq!(group.recordings.total, 1);
        assert_eq!(&group.recordings.items[0].recording.id, recording);
        assert_eq!(group.group.library_count, 1);
    }
    assert_eq!(library.snapshot().await, before);
    assert_eq!(library.catalog.requests(), requests);
    assert_eq!(other.requests(), other_requests);
    assert_eq!(library.origin_requests().await, origin);
}
