#[path = "fixtures/artist_catalog_discovery_service.rs"]
mod discovery;
#[path = "fixtures/artist_catalog_metadata_only.rs"]
mod imported;
mod support;

use axum::http::Method;
use futures_util::FutureExt;
use hito_core::{
    Actor, ArtistAssociationEvidence, ArtistCandidatePreviewAvailability,
    ArtistCandidateSearchAvailability, ArtistCreditRole, InstallPlugin, MetadataProvider,
    OfferPluginService, Services, UpdateMetadataSettings,
};
use hito_plugin_sdk::{ArtistProfileRequest, ArtistSearchRequest};
use serde_json::json;
use std::panic::{AssertUnwindSafe, resume_unwind};
use uuid::Uuid;

#[tokio::test]
async fn an_imported_artist_candidate_is_previewed_before_only_its_recording_is_confirmed() {
    let (_directory, hito, actor, _library, _music) =
        support::library("artist-discovery-review").await;
    let origin = imported::ProviderServer::start().await;
    let mut server = discovery::DiscoveryServer::start().await;
    let result = AssertUnwindSafe(exercise(&hito, &actor, &origin, &server))
        .catch_unwind()
        .await;
    origin.shutdown().await;
    let joined = server.finish().await;
    if let Err(panic) = result {
        resume_unwind(panic);
    }
    assert!(
        joined,
        "The owned discovery fixture must close and join shutdown"
    );
}

async fn exercise(
    hito: &Services,
    actor: &Actor,
    origin: &imported::ProviderServer,
    server: &discovery::DiscoveryServer,
) {
    let imported = imported::Imported::create(hito, actor, origin).await;
    let service = hito
        .plugins
        .offer_service(
            actor,
            OfferPluginService {
                name: "Owned artist discovery".into(),
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
                service_id: service.id,
                service_revision: service.revision,
                bearer_token: Some(discovery::TOKEN.into()),
            },
        )
        .await
        .unwrap();
    let settings = hito.catalog.metadata_settings(actor).await.unwrap();
    let provider = MetadataProvider::HttpPlugin {
        installation_id: installation.id,
    };
    hito.catalog
        .set_metadata_settings(
            actor,
            UpdateMetadataSettings {
                revision: settings.revision,
                enabled: true,
                providers: vec![provider.clone()],
            },
        )
        .await
        .unwrap();
    let context = hito
        .catalog
        .begin_artist_review(actor, &imported.recording_id, provider.clone())
        .await
        .unwrap();
    assert!(context.artist_id == imported.artist_id);
    assert!(context.observation_id == imported.observation_id && context.source_id.is_none());
    assert!(context.metadata_binding.is_some() && context.artist_operation_id.is_some());
    let before = imported.snapshot(hito, actor).await;
    let origin_before = imported::request_counts(origin).await;
    let request = ArtistSearchRequest {
        query: imported::CREDIT.into(),
        cursor: None,
        snapshot: None,
        limit: 2,
    };
    let found = hito
        .catalog
        .search_artist_candidates_until(actor, &context, request, std::future::pending())
        .await
        .unwrap();
    let ArtistCandidateSearchAvailability::Available {
        context: searched_context,
        page,
    } = found
    else {
        panic!("The selected metadata provider must return its candidates");
    };
    assert!(*searched_context == context);
    assert!(
        page.items.len() == 2
            && page.items[0].artist.name == page.items[1].artist.name
            && page.items[0].artist.id == discovery::FIRST_ARTIST
            && page.items[1].artist.id == discovery::SECOND_ARTIST
    );
    let selected = &page.items[1].artist.id;
    assert!(imported.snapshot(hito, actor).await == before);
    assert_eq!(server.requests().len(), 3);

    let preview = hito
        .catalog
        .preview_artist_candidate_until(actor, &context, selected, std::future::pending())
        .await
        .expect("The chosen opaque artist identity must support a nonmutating profile preview");
    let ArtistCandidatePreviewAvailability::Available {
        context: preview_context,
        profile,
    } = preview
    else {
        panic!("The exact selected artist profile must be available for review");
    };
    assert!(*preview_context == context);
    profile
        .validate_for(&ArtistProfileRequest {
            artist_id: selected.clone(),
        })
        .unwrap();
    assert!(profile == discovery::expected_profile());
    assert!(profile.artist.id == discovery::SECOND_ARTIST);
    assert!(
        imported.snapshot(hito, actor).await == before,
        "Preview must preserve private pins, ordered duplicate playlist entries, likes, progress, metadata and source/download absence"
    );
    for recording in [&imported.recording_id, &imported.other_recording_id] {
        assert!(
            hito.catalog
                .recording_artist_associations(actor, recording)
                .await
                .unwrap()
                .is_empty(),
            "Candidate discovery and profile preview must not create association evidence"
        );
    }
    let observed = server.requests();
    assert_eq!(observed.len(), 5);
    assert!(
        observed[3].method == Method::GET
            && observed[3].path == "/prefix/v1/catalog/capabilities"
            && observed[3].body.is_null()
    );
    assert!(
        observed[4].method == Method::POST
            && observed[4].path == "/prefix/v1/catalog/artists/profile"
            && observed[4].body == json!({"artist_id": discovery::SECOND_ARTIST}),
        "Preview must send only the selected provider artist identifier, never local review context or the other namesake"
    );

    let association = hito
        .catalog
        .confirm_artist_review(actor, &preview_context, &profile.artist.id)
        .await
        .expect("Explicit confirmation must retain evidence for exactly the reviewed recording");
    assert!(Uuid::parse_str(&association.id).is_ok() && association.revision == 1);
    assert!(association.account_id == context.account_id);
    assert!(association.library_id == context.library_id);
    assert!(association.recording_id == imported.recording_id);
    assert!(association.artist_id == imported.artist_id);
    assert!(
        association.source_id.is_none() && association.observation_id == imported.observation_id
    );
    assert!(association.role == ArtistCreditRole::RecordingArtist);
    assert!(association.provider == provider && association.provider_artist_id == *selected);
    assert!(association.evidence == ArtistAssociationEvidence::UserConfirmed);
    let confirmed = serde_json::to_value(&association).unwrap();
    let retained = hito
        .catalog
        .recording_artist_associations(actor, &imported.recording_id)
        .await
        .unwrap();
    assert!(retained.len() == 1 && serde_json::to_value(&retained[0]).unwrap() == confirmed);
    assert!(
        hito.catalog
            .recording_artist_associations(actor, &imported.other_recording_id)
            .await
            .unwrap()
            .is_empty(),
        "The other recording with the same credited artist must remain unassociated"
    );
    let duplicate = hito
        .catalog
        .confirm_artist_review(actor, &preview_context, &profile.artist.id)
        .await
        .expect("An identical explicit confirmation must return the retained immutable evidence");
    assert!(serde_json::to_value(duplicate).unwrap() == confirmed);
    assert_eq!(
        hito.catalog
            .recording_artist_associations(actor, &imported.recording_id)
            .await
            .unwrap()
            .len(),
        1
    );
    assert!(
        imported.snapshot(hito, actor).await == before,
        "Confirmation must preserve personal credit/title pins, duplicates, likes, progress and source/download absence without merging either recording"
    );
    assert_eq!(imported::request_counts(origin).await, origin_before);
    assert_eq!(
        server.requests().len(),
        observed.len(),
        "Confirmation and its idempotent retry must need no provider I/O"
    );
    assert_eq!(server.unexpected(), 0);
    for request in observed {
        assert!(
            request
                .headers
                .get("authorization")
                .and_then(|value| value.to_str().ok())
                == Some("Bearer private-artist-discovery-service-token")
        );
        assert!(request.headers.get("cookie").is_none());
    }
}
