#[allow(dead_code)]
#[path = "fixtures/artist_catalog_profiles_library.rs"]
mod library;
#[path = "fixtures/metadata_tag_logs.rs"]
mod logs;
#[allow(dead_code)]
#[path = "fixtures/artist_catalog_profiles_service.rs"]
mod provider;
mod support;

use futures_util::FutureExt;
use hito_core::{ArtistProfileAvailability, MetadataProvider};
use hito_plugin_sdk::Capability;
use serde_json::{Value, json};
use std::panic::{AssertUnwindSafe, resume_unwind};
use tracing::instrument::WithSubscriber;
use uuid::Uuid;

#[tokio::test]
async fn a_missing_catalog_descriptor_preserves_the_connection_and_reviewed_profile() {
    let library = library::Library::prepare().await;
    let mut server = provider::CatalogServer::start().await;
    let result = AssertUnwindSafe(exercise(&library, &server))
        .catch_unwind()
        .await;
    server.release_profile().await;
    let joined = server.finish().await;
    if let Err(panic) = result {
        resume_unwind(panic);
    }
    assert!(joined, "The owned HTTP fixture must join shutdown");
}

async fn exercise(library: &library::Library, server: &provider::CatalogServer) {
    let hito = &library.hito;
    let association = library
        .review_artist(&server.url, provider::ARTIST_ID)
        .await;
    let MetadataProvider::HttpPlugin { installation_id } = &association.provider else {
        panic!("The reviewed association must use the personally approved HTTP service");
    };
    let settings = hito.plugins.settings(&library.actor).await.unwrap();
    let installation = settings
        .installations
        .iter()
        .find(|installation| &installation.id == installation_id)
        .expect("The reviewed provider must remain an active personal installation");
    assert!(
        installation
            .manifest
            .capabilities
            .contains(&Capability::Catalog),
        "The personal installation must approve the Catalog capability"
    );
    assert!(
        settings
            .services
            .iter()
            .any(|service| service.id == installation.service_id),
        "The personally approved installation must retain its active offered service"
    );
    let before = state(library).await;
    let captured = logs::CapturedLogs::default();
    let dispatch = tracing::Dispatch::new(captured.subscriber());
    assert_eq!(
        server.counts().await,
        provider::Counts {
            manifests: 1,
            descriptors: 0,
            profiles: 0,
            unexpected: 0
        }
    );

    server.set_descriptor_missing(true);
    let result = hito
        .catalog
        .recording_artist_profile_until(
            &library.actor,
            &library.recording_id,
            &association.id,
            std::future::pending(),
        )
        .with_subscriber(dispatch.clone())
        .await
        .unwrap();
    assert!(
        matches!(result, ArtistProfileAvailability::Unsupported),
        "A missing optional catalog descriptor must report Unsupported"
    );
    assert_eq!(
        server.counts().await,
        provider::Counts {
            manifests: 1,
            descriptors: 1,
            profiles: 0,
            unexpected: 0
        }
    );
    assert!(
        state(library).await == before,
        "A missing descriptor must retain the exact association, active approval, policy and private state"
    );

    server.set_descriptor_missing(false);
    let result = hito
        .catalog
        .recording_artist_profile_until(
            &library.actor,
            &library.recording_id,
            &association.id,
            std::future::pending(),
        )
        .with_subscriber(dispatch)
        .await
        .unwrap();
    let ArtistProfileAvailability::Available {
        association_id,
        profile,
    } = result
    else {
        panic!("The same connection must retrieve its reviewed profile once the descriptor exists");
    };
    assert!(association_id == association.id && profile == provider::expected_profile());
    assert_eq!(
        server.counts().await,
        provider::Counts {
            manifests: 1,
            descriptors: 2,
            profiles: 1,
            unexpected: 0
        }
    );
    assert!(
        state(library).await == before,
        "Descriptor recovery must preserve the exact association, active approval, policy and private state"
    );
    assert_logs(
        &captured.text(),
        &[
            provider::MISSING_DESCRIPTOR_BODY,
            provider::ARTIST_ID,
            "org.hito.artist-catalog-fixture",
            "Verified provider artist",
            "Provider artist, Verified",
            library::CREDIT,
            "Private reviewed title",
            "Owned album",
            "recording-0.wav",
            "recording-1.wav",
            "/music/",
            &server.url,
            &library.artist_id,
            &library.recording_id,
            &library.source_id,
            &association.id,
            installation_id,
            "artist catalog listener password",
            "Bearer ",
        ],
    );
}

async fn state(library: &library::Library) -> Value {
    let hito = &library.hito;
    let actor = &library.actor;
    json!({
        "library": library.snapshot().await,
        "associations": hito.catalog.recording_artist_associations(actor, &library.recording_id).await.unwrap(),
        "namesake_associations": hito.catalog.recording_artist_associations(actor, &library.other_recording_id).await.unwrap(),
        "policy": hito.catalog.metadata_settings(actor).await.unwrap(),
        "plugins": hito.plugins.settings(actor).await.unwrap(),
    })
}

fn assert_logs(captured: &str, private_values: &[&str]) {
    for private in private_values {
        assert!(
            !captured.contains(private),
            "Active artist profile logs must omit private values and descriptor error bodies"
        );
    }
    let events = captured
        .lines()
        .map(|line| {
            serde_json::from_str::<Value>(line)
                .unwrap_or_else(|_| panic!("Artist profile workflow logs must use structured JSON"))
        })
        .filter(|event| event["fields"]["message"] == "Artist profile read completed")
        .collect::<Vec<_>>();
    assert!(
        events.len() == 2,
        "Descriptor absence and recovery must each log their profile outcome"
    );
    let mut requests = Vec::new();
    for (event, outcome) in events.iter().zip(["unsupported", "available"]) {
        assert!(
            event["fields"]["outcome"] == outcome
                && event["fields"]["elapsed_ms"].as_u64().is_some()
                && event["span"]["operation"] == "artist_profile",
            "Profile outcomes must retain their safe outcome, timing and operation scope"
        );
        let request = event["span"]["request_id"]
            .as_str()
            .expect("Profile outcomes must retain a correlation ID");
        assert!(
            Uuid::parse_str(request).is_ok(),
            "Profile correlation IDs must be generated identifiers"
        );
        requests.push(request);
    }
    assert!(
        requests[0] != requests[1],
        "Separate profile attempts need distinct correlation IDs"
    );
}
