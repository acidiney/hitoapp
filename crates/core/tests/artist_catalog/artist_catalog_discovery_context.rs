use hito_core::test_support::artist_catalog_discovery_service as discovery;
#[path = "../fixtures/artist_catalog_metadata_only.rs"]
mod imported;
use hito_core::test_support::support;

use futures_util::FutureExt;
use hito_core::{
    Action, Actor, ArtistCandidateSearchAvailability, Error, InstallPlugin, MetadataProvider,
    OfferPluginService, Services, UpdateMetadataSettings,
};
use hito_plugin_sdk::ArtistSearchRequest;
use serde_json::json;
use std::panic::{AssertUnwindSafe, resume_unwind};
use uuid::Uuid;

#[tokio::test]
async fn returning_to_the_same_artist_credit_does_not_restore_a_stale_discovery_context() {
    let (_directory, hito, actor, _library, _music) =
        support::library("artist-discovery-aba").await;
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
    let selected = MetadataProvider::HttpPlugin {
        installation_id: installation.id,
    };
    hito.catalog
        .set_metadata_settings(
            actor,
            UpdateMetadataSettings {
                revision: settings.revision,
                enabled: true,
                providers: vec![selected.clone()],
            },
        )
        .await
        .unwrap();
    let captured = hito
        .catalog
        .begin_artist_review(actor, &imported.recording_id, selected.clone())
        .await
        .expect("The first current credit must provide a review context");
    assert!(
        captured.artist_id == imported.artist_id,
        "The captured review must belong to the imported recording's exact account artist"
    );
    assert!(
        captured.observation_id == imported.observation_id && captured.source_id.is_none(),
        "The captured review must retain the imported source-free observation identity"
    );
    let metadata_before = hito
        .catalog
        .recording_metadata(actor, &imported.recording_id, Default::default())
        .await
        .unwrap();
    assert!(captured.credited_name == imported::CREDIT);
    assert!(
        captured.artist_operation_id.is_some()
            && captured.artist_operation_id == metadata_before.artist.operation_id
    );
    let requests_before = server.requests();
    assert!(requests_before.len() == 1 && requests_before[0].path == "/prefix/v1/manifest");
    let origin_before = imported::request_counts(origin).await;

    let recording = Uuid::parse_str(&imported.recording_id).unwrap();
    support::synchronize(
        hito,
        actor,
        vec![
            Action::RecordingArtistSet {
                recording_id: recording,
                artist: "Private intervening performer".into(),
            },
            Action::RecordingArtistSet {
                recording_id: recording,
                artist: imported::CREDIT.into(),
            },
        ],
    )
    .await;
    let metadata_after = hito
        .catalog
        .recording_metadata(actor, &imported.recording_id, Default::default())
        .await
        .unwrap();
    assert!(
        metadata_after.artist.value == imported::CREDIT
            && metadata_after.artist.personal.as_deref() == Some(imported::CREDIT)
    );
    assert!(
        metadata_after.artist.operation_id.is_some()
            && metadata_after.artist.operation_id != captured.artist_operation_id,
        "Returning to the same personal credit must retain the new accepted operation identity"
    );
    assert!(
        serde_json::to_value(&metadata_after.selected_observation).unwrap()
            == serde_json::to_value(&metadata_before.selected_observation).unwrap(),
        "Personal credit edits must preserve the exact provider observation"
    );
    let fresh = hito
        .catalog
        .begin_artist_review(actor, &imported.recording_id, selected)
        .await
        .expect("The restored current credit must allow a new review context");
    assert!(fresh.artist_id == captured.artist_id && fresh.credited_name == captured.credited_name);
    assert!(fresh.artist_operation_id == metadata_after.artist.operation_id);
    let mut expected_fresh = serde_json::to_value(&captured).unwrap();
    expected_fresh["artist_operation_id"] = json!(fresh.artist_operation_id);
    assert!(
        serde_json::to_value(&fresh).unwrap() == expected_fresh,
        "The public A-to-B-to-A workflow must change only the artist operation in its complete review context"
    );
    assert_eq!(server.requests().len(), requests_before.len());
    assert_eq!(imported::request_counts(origin).await, origin_before);
    let after_edits = imported.snapshot(hito, actor).await;
    let request = || ArtistSearchRequest {
        query: imported::CREDIT.into(),
        cursor: None,
        snapshot: None,
        limit: 2,
    };

    let stale = hito
        .catalog
        .search_artist_candidates_until(actor, &captured, request(), std::future::pending())
        .await;
    assert!(
        matches!(stale, Err(Error::Conflict(_))),
        "Equal current names and artist IDs must not authorize a context from before an intervening personal edit"
    );
    assert_eq!(
        server.requests().len(),
        requests_before.len(),
        "A stale review context must be rejected before descriptor or search HTTP"
    );
    assert_eq!(server.unexpected(), 0);
    assert!(
        imported.snapshot(hito, actor).await == after_edits,
        "Rejecting a stale context must preserve the exact post-edit private library state"
    );
    assert_eq!(imported::request_counts(origin).await, origin_before);
    for recording in [&imported.recording_id, &imported.other_recording_id] {
        assert!(
            hito.catalog
                .recording_artist_associations(actor, recording)
                .await
                .unwrap()
                .is_empty(),
            "Stale discovery must not create artist association evidence"
        );
    }

    let result = hito
        .catalog
        .search_artist_candidates_until(actor, &fresh, request(), std::future::pending())
        .await
        .expect("A freshly captured current context must remain usable");
    let ArtistCandidateSearchAvailability::Available { context, page } = result else {
        panic!("The same account must recover discovery by obtaining the current context");
    };
    assert!(serde_json::to_value(context).unwrap() == serde_json::to_value(&fresh).unwrap());
    page.validate_for(&request()).unwrap();
    assert!(
        page.items.len() == 2
            && page.items[0].artist.id == discovery::FIRST_ARTIST
            && page.items[1].artist.id == discovery::SECOND_ARTIST
    );
    assert!(
        imported.snapshot(hito, actor).await == after_edits,
        "A fresh discovery read must preserve the accepted edits, duplicate playlist entries, likes, progress and download state"
    );
    for recording in [&imported.recording_id, &imported.other_recording_id] {
        assert!(
            hito.catalog
                .recording_artist_associations(actor, recording)
                .await
                .unwrap()
                .is_empty()
        );
    }
    assert_eq!(imported::request_counts(origin).await, origin_before);
    let observed = server.requests();
    assert!(
        observed.len() == 3
            && observed[1].path == "/prefix/v1/catalog/discovery/capabilities"
            && observed[2].path == "/prefix/v1/catalog/artists/search"
    );
    assert_eq!(server.unexpected(), 0);
}
