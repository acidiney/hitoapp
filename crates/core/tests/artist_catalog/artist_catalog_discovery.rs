use hito_core::test_support::artist_catalog_discovery_service as discovery;
#[path = "../fixtures/artist_catalog_metadata_only.rs"]
mod imported;
use hito_core::test_support::support;

use axum::http::Method;
use futures_util::FutureExt;
use hito_core::{
    Actor, ArtistCandidateSearchAvailability, InstallPlugin, MetadataProvider, OfferPluginService,
    Services, UpdateMetadataSettings,
};
use hito_plugin_sdk::ArtistSearchRequest;
use serde_json::json;
use std::panic::{AssertUnwindSafe, resume_unwind};
use uuid::Uuid;

#[tokio::test]
async fn imported_artist_discovery_captures_current_review_context_without_mutating_the_library() {
    let (_directory, hito, actor, _library, _music) = support::library("artist-discovery").await;
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
    assert!(joined, "The owned discovery server must join shutdown");
}

async fn exercise(
    hito: &Services,
    actor: &Actor,
    origin: &imported::ProviderServer,
    server: &discovery::DiscoveryServer,
) {
    let imported = imported::Imported::create(hito, actor, origin).await;
    let defaults = hito.catalog.metadata_settings(actor).await.unwrap();
    assert!(defaults.enabled && defaults.providers == vec![MetadataProvider::BuiltinMusicBrainz]);
    let before_default = imported.snapshot(hito, actor).await;
    let origin_before = imported::request_counts(origin).await;
    let builtin = hito
        .catalog
        .begin_artist_review(
            actor,
            &imported.recording_id,
            MetadataProvider::BuiltinMusicBrainz,
        )
        .await
        .expect("The default built-in provider must support a network-free review context");
    assert!(
        builtin.provider == MetadataProvider::BuiltinMusicBrainz && builtin.connection.is_none()
    );
    assert!(builtin.source_id.is_none() && builtin.metadata_binding.is_some());
    assert_eq!(builtin.policy_revision, defaults.revision);
    assert!(
        server.requests().is_empty(),
        "Beginning a review must not fetch any provider metadata"
    );
    assert!(imported.snapshot(hito, actor).await == before_default);
    assert_eq!(imported::request_counts(origin).await, origin_before);

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
                service_id: service.id.clone(),
                service_revision: service.revision,
                bearer_token: Some(discovery::TOKEN.into()),
            },
        )
        .await
        .unwrap();
    let selected = MetadataProvider::HttpPlugin {
        installation_id: installation.id.clone(),
    };
    let policy = hito
        .catalog
        .set_metadata_settings(
            actor,
            UpdateMetadataSettings {
                revision: defaults.revision,
                enabled: true,
                providers: vec![selected.clone()],
            },
        )
        .await
        .unwrap();
    let before = imported.snapshot(hito, actor).await;
    let metadata = hito
        .catalog
        .recording_metadata(actor, &imported.recording_id, Default::default())
        .await
        .unwrap();
    let artists = hito
        .catalog
        .artists(actor, Default::default())
        .await
        .unwrap();
    assert!(artists.total == 1 && artists.items[0].id == imported.artist_id);
    let associations_before = hito
        .catalog
        .recording_artist_associations(actor, &imported.recording_id)
        .await
        .unwrap();
    assert!(associations_before.is_empty());
    let context = hito
        .catalog
        .begin_artist_review(actor, &imported.recording_id, selected.clone())
        .await
        .expect("The selected personal provider must capture the current imported credit");
    assert!(context.account_id == actor.user().id);
    assert!(context.recording_id == imported.recording_id);
    assert!(context.library_id == artists.items[0].library_id);
    assert!(context.artist_id == imported.artist_id && context.credited_name == imported::CREDIT);
    assert!(context.observation_id == imported.observation_id && context.source_id.is_none());
    assert!(context.artist_operation_id == metadata.artist.operation_id);
    assert!(
        context.artist_operation_id.is_some(),
        "A personal artist pin must retain its operation identity"
    );
    assert!(Uuid::parse_str(&context.access_grant_id).is_ok());
    assert!(context.metadata_binding == builtin.metadata_binding);
    let binding = context.metadata_binding.as_ref().unwrap();
    assert!(Uuid::parse_str(&binding.id).is_ok() && binding.revision == 1);
    assert!(context.provider == selected && context.policy_revision == policy.revision);
    let connection = context.connection.as_ref().unwrap();
    assert!(connection.installation_revision == installation.revision);
    assert!(connection.service_id == service.id && connection.service_revision == service.revision);
    let context_before = serde_json::to_value(&context).unwrap();
    assert!(context_before.get("source_id") == Some(&serde_json::Value::Null));
    let admitted = server.requests();
    assert!(admitted.len() == 1 && admitted[0].path == "/prefix/v1/manifest");
    assert!(imported.snapshot(hito, actor).await == before);
    assert_eq!(imported::request_counts(origin).await, origin_before);

    let request = || ArtistSearchRequest {
        query: imported::CREDIT.into(),
        cursor: None,
        snapshot: None,
        limit: 2,
    };
    let result = hito
        .catalog
        .search_artist_candidates_until(actor, &context, request(), std::future::pending())
        .await
        .expect("Artist candidate discovery must succeed for the current reviewed context");
    let ArtistCandidateSearchAvailability::Available {
        context: returned_context,
        page,
    } = result
    else {
        panic!("The selected metadata provider must return artist candidates");
    };
    assert!(serde_json::to_value(&returned_context).unwrap() == context_before);
    page.validate_for(&request())
        .expect("Core must return the provider-neutral candidate contract");
    assert!(
        page.query == imported::CREDIT && page.snapshot.as_deref() == Some(discovery::SNAPSHOT)
    );
    assert!(page.items.len() == 2 && page.total_count == Some(2) && page.next_cursor.is_none());
    assert!(page.items[0].artist.name == page.items[1].artist.name);
    assert!(
        page.items[0].artist.id == discovery::FIRST_ARTIST
            && page.items[1].artist.id == discovery::SECOND_ARTIST
    );
    assert!(page.items[1].kind.is_none() && page.items[1].disambiguation.is_none());
    assert!(
        imported.snapshot(hito, actor).await == before,
        "Discovery must preserve private pins, ordered duplicate playlist entries, likes, progress, observations and download state"
    );
    for recording in [&imported.recording_id, &imported.other_recording_id] {
        assert!(
            hito.catalog
                .recording_artist_associations(actor, recording)
                .await
                .unwrap()
                .is_empty(),
            "Candidate names and valid provider identities must not create reviewed associations"
        );
    }
    assert_eq!(imported::request_counts(origin).await, origin_before);
    assert_eq!(
        server.unexpected(),
        0,
        "Discovery must not request profiles, audio, downloads or unrelated plugin operations"
    );
    let observed = server.requests();
    assert_eq!(observed.len(), 3);
    assert!(observed[0].method == Method::GET && observed[0].path == "/prefix/v1/manifest");
    assert!(
        observed[1].method == Method::GET
            && observed[1].path == "/prefix/v1/catalog/discovery/capabilities"
    );
    assert!(
        observed[2].method == Method::POST
            && observed[2].path == "/prefix/v1/catalog/artists/search"
    );
    assert!(observed[0].body.is_null() && observed[1].body.is_null());
    assert!(
        observed[2].body
            == json!({"query": imported::CREDIT, "cursor": null, "snapshot": null, "limit": 2}),
        "The provider must receive only the SDK query envelope, never local review context or library data"
    );
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
