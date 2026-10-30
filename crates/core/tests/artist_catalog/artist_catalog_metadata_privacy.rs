#[path = "../fixtures/artist_catalog_metadata_only.rs"]
mod imported;
use hito_core::test_support::artist_catalog_profiles_service as provider;
use hito_core::test_support::support;

use futures_util::FutureExt;
use hito_core::{
    Actor, ArtistAssociation, ArtistCreditRole, ArtistProfileAvailability,
    AssociateRecordingArtist, Credentials, Error, InstallPlugin, MetadataProvider,
    OfferPluginService, PluginService, Services, UpdateMetadataSettings,
};
use std::panic::{AssertUnwindSafe, resume_unwind};

#[tokio::test]
async fn another_accounts_approved_catalog_cannot_access_private_imported_artist_evidence() {
    let (_directory, hito, owner, _library, _music) =
        support::library("artist-catalog-privacy").await;
    let origin = imported::ProviderServer::start().await;
    let mut server = provider::CatalogServer::start().await;
    let result = AssertUnwindSafe(exercise(&hito, &owner, &origin, &server))
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
    owner: &Actor,
    origin: &imported::ProviderServer,
    server: &provider::CatalogServer,
) {
    let imported = imported::Imported::create(hito, owner, origin).await;
    hito.identity
        .create_user(
            owner,
            Credentials {
                username: "catalog-outsider".into(),
                password: "owned catalog outsider password".into(),
            },
        )
        .await
        .unwrap();
    let session = hito
        .identity
        .login(Credentials {
            username: "catalog-outsider".into(),
            password: "owned catalog outsider password".into(),
        })
        .await
        .unwrap();
    let outsider = hito.identity.authenticate(&session.token).await.unwrap();
    let service = hito
        .plugins
        .offer_service(
            owner,
            OfferPluginService {
                name: "Owned artist catalog".into(),
                base_url: server.url.clone(),
            },
        )
        .await
        .unwrap();
    let (owner_provider, owner_revision) = connect(hito, owner, &service).await;
    let (outsider_provider, outsider_revision) = connect(hito, &outsider, &service).await;
    assert!(
        owner_provider != outsider_provider,
        "Each account must have its own approved catalog installation"
    );
    let association = hito
        .artist_catalog
        .associate_recording_artist(
            owner,
            &imported.recording_id,
            request(&imported, owner_provider, owner_revision),
        )
        .await
        .unwrap();
    assert_owner_profile(hito, owner, &association).await;
    let before = imported.snapshot(hito, owner).await;
    let evidence_before = serde_json::to_value(&association).unwrap();
    let counts_before = server.counts().await;
    assert_eq!(
        counts_before,
        provider::Counts {
            manifests: 2,
            descriptors: 1,
            profiles: 1,
            unexpected: 0,
        }
    );
    let origin_before = imported::request_counts(origin).await;

    let listed = hito
        .artist_catalog
        .recording_artist_associations(&outsider, &imported.recording_id)
        .await;
    assert!(
        matches!(listed, Err(Error::NotFound)),
        "Another account must not list a private recording's artist evidence"
    );
    let reviewed = hito
        .artist_catalog
        .associate_recording_artist(
            &outsider,
            &imported.recording_id,
            request(&imported, outsider_provider, outsider_revision),
        )
        .await;
    assert!(
        matches!(reviewed, Err(Error::NotFound)),
        "Catalog approval must not authorize review of another account's observation"
    );
    let read = hito
        .artist_catalog
        .recording_artist_profile_until(
            &outsider,
            &imported.recording_id,
            &association.id,
            std::future::pending(),
        )
        .await;
    assert!(
        matches!(read, Err(Error::NotFound)),
        "Known recording and association IDs must not authorize a private profile read"
    );
    assert_eq!(server.counts().await, counts_before);
    assert_eq!(imported::request_counts(origin).await, origin_before);

    let retained = hito
        .artist_catalog
        .recording_artist_associations(owner, &imported.recording_id)
        .await
        .unwrap();
    assert!(
        retained.len() == 1 && serde_json::to_value(&retained[0]).unwrap() == evidence_before,
        "Denied cross-account requests must preserve the owner's exact association evidence"
    );
    assert!(
        imported.snapshot(hito, owner).await == before,
        "Denied cross-account requests must preserve every owner library and collection value"
    );
    assert_eq!(server.counts().await, counts_before);
    assert_owner_profile(hito, owner, &association).await;
    assert!(
        imported.snapshot(hito, owner).await == before,
        "The owner's profile must remain readable without changing private library state"
    );
    assert_eq!(imported::request_counts(origin).await, origin_before);
    assert!(server.counts().await.unexpected == 0);
}

async fn connect(
    hito: &Services,
    actor: &Actor,
    service: &PluginService,
) -> (MetadataProvider, i64) {
    let installation = hito
        .plugins
        .install(
            actor,
            InstallPlugin {
                accept_manifest_digest: None,
                service_id: service.id.clone(),
                service_revision: service.revision,
                bearer_token: None,
            },
        )
        .await
        .unwrap();
    let current = hito.artist_catalog.metadata_settings(actor).await.unwrap();
    let selected = MetadataProvider::HttpPlugin {
        installation_id: installation.id,
    };
    let policy = hito
        .artist_catalog
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
    (selected, policy.revision)
}

fn request(
    imported: &imported::Imported,
    provider: MetadataProvider,
    revision: i64,
) -> AssociateRecordingArtist {
    AssociateRecordingArtist {
        artist_id: imported.artist_id.clone(),
        source_id: None,
        observation_id: imported.observation_id.clone(),
        role: ArtistCreditRole::RecordingArtist,
        provider,
        provider_artist_id: provider::ARTIST_ID.into(),
        policy_revision: revision,
    }
}

async fn assert_owner_profile(hito: &Services, owner: &Actor, association: &ArtistAssociation) {
    let result = hito
        .artist_catalog
        .recording_artist_profile_until(
            owner,
            &association.recording_id,
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
        panic!("The owner must retain authorized access to the reviewed artist profile");
    };
    assert!(association_id == association.id && profile == provider::expected_profile());
}
