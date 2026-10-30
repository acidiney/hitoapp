#[path = "../fixtures/artist_catalog_profiles_library.rs"]
mod library;
use hito_core::test_support::artist_catalog_profiles_service as provider;
use hito_core::test_support::support;

use futures_util::FutureExt;
use hito_core::{
    ArtistAssociationEvidence, ArtistCreditRole, ArtistProfileAvailability,
    AssociateRecordingArtist, InstallPlugin, MetadataProvider, OfferPluginService,
    UpdateMetadataSettings,
};
use std::panic::{AssertUnwindSafe, resume_unwind};
use std::time::Duration;
use tokio::{task::JoinSet, time::timeout};

#[tokio::test]
async fn a_reviewed_profile_belongs_only_to_its_observed_recording_credit() {
    let library = library::Library::prepare().await;
    let mut server = provider::CatalogServer::start().await;
    let result = AssertUnwindSafe(exercise(&library, &server))
        .catch_unwind()
        .await;
    let joined = server.finish().await;
    if let Err(panic) = result {
        resume_unwind(panic);
    }
    assert!(joined, "The owned HTTP fixture must join shutdown");
}

async fn exercise(library: &library::Library, server: &provider::CatalogServer) {
    let hito = &library.hito;
    let actor = &library.actor;
    let defaults = hito.artist_catalog.metadata_settings(actor).await.unwrap();
    assert!(defaults.enabled);
    assert_eq!(
        defaults.providers,
        vec![MetadataProvider::BuiltinMusicBrainz]
    );
    assert_eq!(
        server.counts().await,
        provider::Counts {
            manifests: 0,
            descriptors: 0,
            profiles: 0,
            unexpected: 0,
        }
    );
    let service = hito
        .plugins
        .offer_service(
            &library.admin,
            OfferPluginService {
                name: "Owned artist catalog".into(),
                base_url: server.url.clone(),
            },
        )
        .await
        .unwrap();
    let connection = hito
        .plugins
        .install(
            actor,
            InstallPlugin {
                accept_manifest_digest: None,
                service_id: service.id,
                service_revision: service.revision,
                bearer_token: None,
            },
        )
        .await
        .unwrap();
    let connected_counts = server.counts().await;
    assert_eq!(connected_counts.manifests, 1);
    let unchanged = hito.artist_catalog.metadata_settings(actor).await.unwrap();
    assert_eq!(unchanged.providers, defaults.providers);
    assert_eq!(server.counts().await, connected_counts);
    let provider = MetadataProvider::HttpPlugin {
        installation_id: connection.id,
    };
    let settings = hito
        .artist_catalog
        .set_metadata_settings(
            actor,
            UpdateMetadataSettings {
                revision: defaults.revision,
                enabled: true,
                providers: vec![provider.clone()],
            },
        )
        .await
        .unwrap();
    assert_eq!(settings.providers, vec![provider.clone()]);
    for recording in [&library.recording_id, &library.other_recording_id] {
        assert!(
            hito.artist_catalog
                .recording_artist_associations(actor, recording)
                .await
                .unwrap()
                .is_empty()
        );
    }
    let before = library.snapshot().await;
    let association = hito
        .artist_catalog
        .associate_recording_artist(
            actor,
            &library.recording_id,
            AssociateRecordingArtist {
                artist_id: library.artist_id.clone(),
                source_id: Some(library.source_id.clone()),
                observation_id: library.observation_id.clone(),
                role: ArtistCreditRole::RecordingArtist,
                provider: provider.clone(),
                provider_artist_id: provider::ARTIST_ID.into(),
                policy_revision: settings.revision,
            },
        )
        .await
        .unwrap();
    assert_eq!(association.account_id, actor.user().id);
    assert_eq!(association.library_id, library.library_id);
    assert_eq!(association.recording_id, library.recording_id);
    assert_eq!(association.artist_id, library.artist_id);
    assert_eq!(association.source_id, Some(library.source_id.clone()));
    assert_eq!(association.observation_id, library.observation_id);
    assert_eq!(association.role, ArtistCreditRole::RecordingArtist);
    assert_eq!(association.provider, provider);
    assert_eq!(association.provider_artist_id, provider::ARTIST_ID);
    assert_eq!(
        association.evidence,
        ArtistAssociationEvidence::UserConfirmed
    );
    let result = hito
        .artist_catalog
        .recording_artist_profile_until(
            actor,
            &library.recording_id,
            &association.id,
            std::future::pending(),
        )
        .await
        .unwrap();
    let ArtistProfileAvailability::Available { profile, .. } = result else {
        panic!("The explicitly reviewed credit must have an available provider profile");
    };
    assert!(profile == provider::expected_profile());
    let associations = hito
        .artist_catalog
        .recording_artist_associations(actor, &library.recording_id)
        .await
        .unwrap();
    assert_eq!(associations.len(), 1);
    assert_eq!(associations[0].id, association.id);
    assert_eq!(
        associations[0].evidence,
        ArtistAssociationEvidence::UserConfirmed
    );
    assert!(
        hito.artist_catalog
            .recording_artist_associations(actor, &library.other_recording_id)
            .await
            .unwrap()
            .is_empty()
    );
    assert_eq!(library.snapshot().await, before);
    let counts = server.counts().await;
    assert!(counts.descriptors > 0);
    assert!(counts.profiles > 0);
    assert_eq!(counts.unexpected, 0);
    assert!(
        server
            .profile_requests()
            .await
            .iter()
            .all(|id| id == provider::ARTIST_ID)
    );
}

#[tokio::test]
async fn disabled_enrichment_retains_the_reviewed_credit_without_requesting_its_provider() {
    let library = library::Library::prepare().await;
    let mut server = provider::CatalogServer::start().await;
    let result = AssertUnwindSafe(exercise_disabled(&library, &server))
        .catch_unwind()
        .await;
    let joined = server.finish().await;
    if let Err(panic) = result {
        resume_unwind(panic);
    }
    assert!(joined, "The owned HTTP fixture must join shutdown");
}

async fn exercise_disabled(library: &library::Library, server: &provider::CatalogServer) {
    let hito = &library.hito;
    let actor = &library.actor;
    let association = library
        .review_artist(&server.url, provider::ARTIST_ID)
        .await;
    let available = hito
        .artist_catalog
        .recording_artist_profile_until(
            actor,
            &library.recording_id,
            &association.id,
            std::future::pending(),
        )
        .await
        .unwrap();
    let ArtistProfileAvailability::Available { profile, .. } = available else {
        panic!("The reviewed profile must be available before enrichment is disabled");
    };
    assert!(profile == provider::expected_profile());
    let before = library.snapshot().await;
    let associations_before = serde_json::to_value(
        hito.artist_catalog
            .recording_artist_associations(actor, &library.recording_id)
            .await
            .unwrap(),
    )
    .unwrap();
    let settings = hito.artist_catalog.metadata_settings(actor).await.unwrap();
    let counts_before = server.counts().await;
    let disabled = hito
        .artist_catalog
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
    assert!(!disabled.enabled);
    assert!(disabled.revision > settings.revision);
    let result = hito
        .artist_catalog
        .recording_artist_profile_until(
            actor,
            &library.recording_id,
            &association.id,
            std::future::pending(),
        )
        .await
        .unwrap();
    assert!(matches!(result, ArtistProfileAvailability::Disabled));
    assert_eq!(server.counts().await, counts_before);
    let retained = serde_json::to_value(
        hito.artist_catalog
            .recording_artist_associations(actor, &library.recording_id)
            .await
            .unwrap(),
    )
    .unwrap();
    assert_eq!(retained, associations_before);
    assert_eq!(library.snapshot().await, before);
    assert_eq!(server.counts().await, counts_before);
    let enabled = hito
        .artist_catalog
        .set_metadata_settings(
            actor,
            UpdateMetadataSettings {
                revision: disabled.revision,
                enabled: true,
                providers: disabled.providers,
            },
        )
        .await
        .unwrap();
    assert!(enabled.enabled);
    assert!(enabled.revision > disabled.revision);
    let available = hito
        .artist_catalog
        .recording_artist_profile_until(
            actor,
            &library.recording_id,
            &association.id,
            std::future::pending(),
        )
        .await
        .unwrap();
    let ArtistProfileAvailability::Available {
        association_id,
        profile,
    } = available
    else {
        panic!("Re-enabling enrichment must restore access to the same reviewed profile");
    };
    assert_eq!(association_id, association.id);
    assert!(profile == provider::expected_profile());
    let retained = serde_json::to_value(
        hito.artist_catalog
            .recording_artist_associations(actor, &library.recording_id)
            .await
            .unwrap(),
    )
    .unwrap();
    assert_eq!(retained, associations_before);
    assert!(
        hito.artist_catalog
            .recording_artist_associations(actor, &library.other_recording_id,)
            .await
            .unwrap()
            .is_empty()
    );
    assert_eq!(library.snapshot().await, before);
    assert_eq!(server.counts().await.unexpected, 0);
}

#[tokio::test]
async fn a_regranted_library_does_not_restore_an_inflight_profile_authority() {
    let library = library::Library::prepare().await;
    let mut server = provider::CatalogServer::start().await;
    let mut reads = JoinSet::new();
    let result = AssertUnwindSafe(exercise_revocation(&library, &server, &mut reads))
        .catch_unwind()
        .await;
    server.release_profile().await;
    reads.shutdown().await;
    let joined = server.finish().await;
    if let Err(panic) = result {
        resume_unwind(panic);
    }
    assert!(joined, "The owned HTTP fixture must join shutdown");
}

async fn exercise_revocation(
    library: &library::Library,
    server: &provider::CatalogServer,
    reads: &mut JoinSet<hito_core::Result<ArtistProfileAvailability>>,
) {
    let hito = &library.hito;
    let actor = &library.actor;
    let association = library
        .review_artist(&server.url, provider::ARTIST_ID)
        .await;
    let before = library.snapshot().await;
    let associations_before = serde_json::to_value(
        hito.artist_catalog
            .recording_artist_associations(actor, &library.recording_id)
            .await
            .unwrap(),
    )
    .unwrap();
    let gate = server.hold_profile().await;
    let artist_catalog = hito.artist_catalog.clone();
    let request_actor = actor.clone();
    let recording_id = library.recording_id.clone();
    let association_id = association.id.clone();
    reads.spawn(async move {
        artist_catalog
            .recording_artist_profile_until(
                &request_actor,
                &recording_id,
                &association_id,
                std::future::pending(),
            )
            .await
    });
    timeout(Duration::from_secs(3), gate.entered())
        .await
        .expect("The owned provider must admit the profile request before access changes");
    let admitted = server.counts().await;
    assert_eq!(
        admitted,
        provider::Counts {
            manifests: 1,
            descriptors: 1,
            profiles: 1,
            unexpected: 0,
        }
    );
    let access_changed = timeout(Duration::from_secs(3), async {
        hito.identity
            .set_library_access(&library.admin, &library.library_id, &actor.user().id, false)
            .await?;
        hito.identity
            .set_library_access(&library.admin, &library.library_id, &actor.user().id, true)
            .await?;
        Ok::<(), hito_core::Error>(())
    })
    .await;
    assert!(
        matches!(access_changed, Ok(Ok(()))),
        "Access revocation and regrant must complete while the provider response is held"
    );
    assert_eq!(server.counts().await, admitted);
    server.release_profile().await;
    let result = timeout(Duration::from_secs(3), reads.join_next())
        .await
        .expect("The owned profile read must finish after its provider is released")
        .expect("The profile read must remain owned until it is joined")
        .expect("The profile read task must not panic");
    assert!(
        matches!(result, Err(hito_core::Error::Conflict(_))),
        "Regranting membership must not authorize a response captured under the revoked grant"
    );
    assert_eq!(server.counts().await, admitted);
    let retained = serde_json::to_value(
        hito.artist_catalog
            .recording_artist_associations(actor, &library.recording_id)
            .await
            .unwrap(),
    )
    .unwrap();
    assert!(
        retained == associations_before,
        "Reviewed association evidence must survive revocation"
    );
    assert!(
        library.snapshot().await == before,
        "Private library and collection state must survive revocation"
    );
    assert_eq!(server.counts().await, admitted);
}
