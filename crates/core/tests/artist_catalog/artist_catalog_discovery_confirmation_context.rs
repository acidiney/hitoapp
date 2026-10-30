use hito_core::test_support::artist_catalog_discovery_service as discovery;
#[path = "../fixtures/artist_catalog_metadata_only.rs"]
mod imported;
use hito_core::test_support::support;

use axum::http::Method;
use futures_util::FutureExt;
use hito_core::{
    Action, Actor, ArtistAssociationEvidence, ArtistCandidatePreviewAvailability, Error,
    InstallPlugin, MetadataProvider, OfferPluginService, Services, UpdateMetadataSettings,
};
use serde_json::json;
use std::panic::{AssertUnwindSafe, resume_unwind};
use uuid::Uuid;

#[tokio::test]
async fn returning_to_the_same_artist_credit_does_not_authorize_an_old_preview_confirmation() {
    let (_directory, hito, actor, _library, _music) =
        support::library("artist-confirmation-aba").await;
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
        "The owned discovery fixture must close and join even on failure"
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
                accept_manifest_digest: None,
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
    let captured = hito
        .catalog
        .begin_artist_review(actor, &imported.recording_id, provider.clone())
        .await
        .unwrap();
    assert!(captured.artist_id == imported.artist_id);
    assert!(captured.observation_id == imported.observation_id && captured.source_id.is_none());
    let before_preview = imported.snapshot(hito, actor).await;
    let origin_before = imported::request_counts(origin).await;
    let preview = hito
        .catalog
        .preview_artist_candidate_until(
            actor,
            &captured,
            discovery::SECOND_ARTIST,
            std::future::pending(),
        )
        .await
        .unwrap();
    let ArtistCandidatePreviewAvailability::Available {
        context: preview_context,
        profile,
    } = preview
    else {
        panic!("The exact provider profile must be successfully previewed before editing");
    };
    assert!(*preview_context == captured && profile == discovery::expected_profile());
    assert!(imported.snapshot(hito, actor).await == before_preview);
    assert_eq!(server.requests().len(), 3);

    let recording = Uuid::parse_str(&imported.recording_id).unwrap();
    support::synchronize(
        hito,
        actor,
        vec![
            Action::RecordingArtistSet {
                recording_id: recording,
                artist: "Private intervening performer after preview".into(),
            },
            Action::RecordingArtistSet {
                recording_id: recording,
                artist: imported::CREDIT.into(),
            },
        ],
    )
    .await;
    let metadata = hito
        .catalog
        .recording_metadata(actor, &imported.recording_id, Default::default())
        .await
        .unwrap();
    assert!(
        metadata.artist.value == imported::CREDIT
            && metadata.artist.personal.as_deref() == Some(imported::CREDIT)
            && metadata.artist.operation_id.is_some()
            && metadata.artist.operation_id != captured.artist_operation_id,
        "The accepted A-to-B-to-A edit must restore the visible name but retain its new operation"
    );
    let after_edits = imported.snapshot(hito, actor).await;
    let stale = hito
        .catalog
        .confirm_artist_review(actor, &preview_context, &profile.artist.id)
        .await;
    assert!(
        matches!(stale, Err(Error::Conflict(_))),
        "A successful old preview must not authorize confirmation after an intervening accepted edit"
    );
    assert_eq!(server.requests().len(), 3);
    assert_eq!(imported::request_counts(origin).await, origin_before);
    assert!(
        imported.snapshot(hito, actor).await == after_edits,
        "Rejecting confirmation must publish no collection events and preserve all post-edit private state"
    );
    for recording in [&imported.recording_id, &imported.other_recording_id] {
        assert!(
            hito.catalog
                .recording_artist_associations(actor, recording)
                .await
                .unwrap()
                .is_empty(),
            "Stale confirmation must retain no association for either namesake"
        );
    }

    let fresh = hito
        .catalog
        .begin_artist_review(actor, &imported.recording_id, provider.clone())
        .await
        .unwrap();
    let mut expected = serde_json::to_value(&captured).unwrap();
    expected["artist_operation_id"] = json!(metadata.artist.operation_id);
    assert!(
        serde_json::to_value(&fresh).unwrap() == expected,
        "Only the current artist operation must differ in the complete fresh review context"
    );
    assert_eq!(server.requests().len(), 3);
    let preview = hito
        .catalog
        .preview_artist_candidate_until(
            actor,
            &fresh,
            discovery::SECOND_ARTIST,
            std::future::pending(),
        )
        .await
        .unwrap();
    let ArtistCandidatePreviewAvailability::Available {
        context: fresh_preview,
        profile,
    } = preview
    else {
        panic!("A fresh current context must recover the exact provider profile preview");
    };
    assert!(*fresh_preview == fresh && profile == discovery::expected_profile());
    let association = hito
        .catalog
        .confirm_artist_review(actor, &fresh_preview, &profile.artist.id)
        .await
        .expect("A new preview with the current artist operation must allow confirmation");
    assert!(
        association.account_id == fresh.account_id
            && association.recording_id == imported.recording_id
            && association.artist_id == imported.artist_id
            && association.observation_id == imported.observation_id
            && association.source_id.is_none()
            && association.provider == provider
            && association.provider_artist_id == discovery::SECOND_ARTIST
            && association.evidence == ArtistAssociationEvidence::UserConfirmed
    );
    let retained = hito
        .catalog
        .recording_artist_associations(actor, &imported.recording_id)
        .await
        .unwrap();
    assert!(
        retained.len() == 1
            && serde_json::to_value(&retained[0]).unwrap()
                == serde_json::to_value(association).unwrap()
    );
    assert!(
        hito.catalog
            .recording_artist_associations(actor, &imported.other_recording_id)
            .await
            .unwrap()
            .is_empty()
    );
    assert!(imported.snapshot(hito, actor).await == after_edits);
    assert_eq!(imported::request_counts(origin).await, origin_before);
    let observed = server.requests();
    assert_eq!(observed.len(), 5);
    assert!(observed[0].method == Method::GET && observed[0].path == "/prefix/v1/manifest");
    let (pairs, remainder) = observed[1..].as_chunks::<2>();
    assert!(remainder.is_empty());
    for pair in pairs {
        assert!(
            pair[0].method == Method::GET
                && pair[0].path == "/prefix/v1/catalog/capabilities"
                && pair[0].body.is_null()
        );
        assert!(
            pair[1].method == Method::POST
                && pair[1].path == "/prefix/v1/catalog/artists/profile"
                && pair[1].body == json!({"artist_id": discovery::SECOND_ARTIST})
        );
    }
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
    assert_eq!(server.unexpected(), 0);
}
