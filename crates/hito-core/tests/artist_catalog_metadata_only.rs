#[path = "fixtures/artist_catalog_metadata_only.rs"]
mod imported;
#[allow(dead_code)]
#[path = "fixtures/artist_catalog_profiles_service.rs"]
mod provider;
mod support;

use futures_util::FutureExt;
use hito_core::{
    Actor, ArtistAssociationEvidence, ArtistCreditRole, ArtistProfileAvailability,
    AssociateRecordingArtist, InstallPlugin, MetadataProvider, OfferPluginService, Services,
    UpdateMetadataSettings,
};
use std::panic::{AssertUnwindSafe, resume_unwind};

#[tokio::test]
async fn imported_metadata_can_have_a_reviewed_artist_profile_before_audio_adoption() {
    let (_directory, hito, actor, _library, _music) =
        support::library("artist-catalog-metadata-only").await;
    let origin = imported::ProviderServer::start().await;
    let mut server = provider::CatalogServer::start().await;
    let result = AssertUnwindSafe(exercise(&hito, &actor, &origin, &server))
        .catch_unwind()
        .await;
    origin.shutdown().await;
    let joined = server.finish().await;
    if let Err(panic) = result {
        resume_unwind(panic);
    }
    assert!(joined, "The owned catalog HTTP fixture must join shutdown");
}

async fn exercise(
    hito: &Services,
    actor: &Actor,
    origin: &imported::ProviderServer,
    server: &provider::CatalogServer,
) {
    let imported = imported::Imported::create(hito, actor, origin).await;
    let service = hito
        .plugins
        .offer_service(
            actor,
            OfferPluginService {
                name: "Owned artist catalog".into(),
                base_url: server.url.clone(),
            },
        )
        .await
        .unwrap();
    let installation = hito
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
    let current = hito.catalog.metadata_settings(actor).await.unwrap();
    let selected = MetadataProvider::HttpPlugin {
        installation_id: installation.id,
    };
    let policy = hito
        .catalog
        .set_metadata_settings(
            actor,
            UpdateMetadataSettings {
                revision: current.revision,
                enabled: true,
                providers: vec![selected.clone()],
            },
        )
        .await
        .unwrap();
    let before = imported.snapshot(hito, actor).await;
    let origin_before = imported::request_counts(origin).await;
    assert_eq!(
        origin_before,
        vec![
            ("v1/manifest", 1),
            ("v1/playlists/detail", 2),
            ("v1/playlists/items", 1),
            ("v1/search", 0),
            ("v1/recordings/detail", 0),
            ("v1/recordings/metadata", 0),
            ("v1/sources", 0),
            ("v1/audio", 0),
        ]
    );
    let request = || AssociateRecordingArtist {
        artist_id: imported.artist_id.clone(),
        source_id: None,
        observation_id: imported.observation_id.clone(),
        role: ArtistCreditRole::RecordingArtist,
        provider: selected.clone(),
        provider_artist_id: provider::ARTIST_ID.into(),
        policy_revision: policy.revision,
    };
    let association = hito
        .catalog
        .associate_recording_artist(actor, &imported.recording_id, request())
        .await
        .unwrap();
    assert!(association.artist_id == imported.artist_id);
    assert!(association.observation_id == imported.observation_id);
    assert!(association.evidence == ArtistAssociationEvidence::UserConfirmed);
    let evidence = serde_json::to_value(&association).unwrap();
    assert!(
        evidence.get("source_id") == Some(&serde_json::Value::Null),
        "Metadata-only review must retain absent source provenance"
    );
    let result = hito
        .catalog
        .recording_artist_profile_until(
            actor,
            &imported.recording_id,
            &association.id,
            std::future::pending(),
        )
        .await
        .unwrap();
    let ArtistProfileAvailability::Available {
        association_id,
        profile,
    } = result
    else {
        panic!(
            "A reviewed metadata-only recording must provide its artist profile before audio adoption"
        );
    };
    assert!(association_id == association.id);
    assert!(profile == provider::expected_profile());
    let requests_before_repeat = server.counts().await;
    assert_eq!(
        requests_before_repeat,
        provider::Counts {
            manifests: 1,
            descriptors: 1,
            profiles: 1,
            unexpected: 0
        }
    );
    let repeated = hito
        .catalog
        .associate_recording_artist(actor, &imported.recording_id, request())
        .await
        .unwrap();
    assert!(
        repeated.id == association.id,
        "An identical metadata-only review must retain its association ID"
    );
    assert!(
        serde_json::to_value(repeated).unwrap() == evidence,
        "An identical metadata-only review must retain its exact evidence"
    );
    assert_eq!(server.counts().await, requests_before_repeat);
    let associations = hito
        .catalog
        .recording_artist_associations(actor, &imported.recording_id)
        .await
        .unwrap();
    assert!(associations.len() == 1 && associations[0].id == association.id);
    assert!(
        hito.catalog
            .recording_artist_associations(actor, &imported.other_recording_id)
            .await
            .unwrap()
            .is_empty(),
        "The unrelated imported recording must not inherit an artist association"
    );
    assert!(
        imported.snapshot(hito, actor).await == before,
        "Artist metadata must preserve provider observations, ordered duplicate entries and all private library state"
    );
    assert_eq!(imported::request_counts(origin).await, origin_before);
    assert_eq!(server.counts().await, requests_before_repeat);
}
