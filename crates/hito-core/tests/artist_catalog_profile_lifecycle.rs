#[allow(dead_code)]
#[path = "fixtures/artist_catalog_profiles_library.rs"]
mod library;
use hito_core::test_support::artist_catalog_profiles_service as provider;
use hito_core::test_support::metadata_tag_logs as logs;
use hito_core::test_support::support;

use futures_util::FutureExt;
use hito_core::{ArtistProfileAvailability, Credentials, Error};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{
    panic::{AssertUnwindSafe, resume_unwind},
    time::Duration,
};
use tokio::{sync::oneshot, task::JoinSet, time::timeout};
use tracing::instrument::WithSubscriber;
use uuid::Uuid;

#[tokio::test]
async fn explicit_profile_cancellation_releases_the_same_accounts_provider_capacity() {
    let library = library::Library::prepare().await;
    let mut server = provider::CatalogServer::start().await;
    let mut reads = JoinSet::new();
    let result = AssertUnwindSafe(exercise(&library, &server, &mut reads))
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

async fn exercise(
    library: &library::Library,
    server: &provider::CatalogServer,
    reads: &mut JoinSet<hito_core::Result<ArtistProfileAvailability>>,
) {
    let hito = &library.hito;
    let association = library
        .review_artist(&server.url, provider::ARTIST_ID)
        .await;
    let session = hito
        .identity
        .login(Credentials {
            username: "listener".into(),
            password: "artist catalog listener password".into(),
        })
        .await
        .unwrap();
    let actor = hito.identity.authenticate(&session.token).await.unwrap();
    let before = library.snapshot().await;
    let evidence = serde_json::to_value(&association).unwrap();
    let captured = logs::CapturedLogs::default();
    let dispatch = tracing::Dispatch::new(captured.subscriber());
    let gate = server.hold_profile().await;
    let (cancel, cancelled) = oneshot::channel();
    let catalog = hito.catalog.clone();
    let request_actor = actor.clone();
    let recording_id = library.recording_id.clone();
    let association_id = association.id.clone();
    reads.spawn(
        async move {
            catalog
                .recording_artist_profile_until(
                    &request_actor,
                    &recording_id,
                    &association_id,
                    async {
                        let _ = cancelled.await;
                    },
                )
                .await
        }
        .with_subscriber(dispatch.clone()),
    );
    timeout(Duration::from_secs(3), gate.entered())
        .await
        .expect("The real profile request must be held before explicit cancellation");
    let admitted = server.counts().await;
    assert_eq!(
        admitted,
        provider::Counts {
            manifests: 1,
            descriptors: 1,
            profiles: 1,
            unexpected: 0
        }
    );
    cancel
        .send(())
        .expect("The explicit cancellation receiver must remain owned");
    let result = timeout(Duration::from_secs(3), reads.join_next())
        .await
        .expect("Explicit cancellation must finish while the provider response remains held")
        .expect("The cancelled read must remain owned until joined")
        .expect("The explicitly cancelled read must not panic");
    assert!(
        matches!(result, Ok(ArtistProfileAvailability::Cancelled)),
        "The public cancellation boundary must report Cancelled"
    );
    assert_eq!(server.counts().await, admitted);
    server.release_profile().await;
    timeout(Duration::from_secs(3), gate.retired())
        .await
        .expect("The held provider handler must retire before its account retries");
    let result = timeout(
        Duration::from_secs(3),
        hito.catalog
            .recording_artist_profile_until(
                &actor,
                &library.recording_id,
                &association.id,
                std::future::pending(),
            )
            .with_subscriber(dispatch),
    )
    .await
    .expect("The same account must be admitted again after explicit cancellation")
    .unwrap();
    let ArtistProfileAvailability::Available {
        association_id,
        profile,
    } = result
    else {
        panic!("The same account must retrieve its reviewed profile after cancellation");
    };
    assert!(association_id == association.id && profile == provider::expected_profile());
    assert_eq!(
        server.counts().await,
        provider::Counts {
            manifests: 1,
            descriptors: 2,
            profiles: 2,
            unexpected: 0
        }
    );
    let retained = hito
        .catalog
        .recording_artist_associations(&actor, &library.recording_id)
        .await
        .unwrap();
    assert!(
        retained.len() == 1 && serde_json::to_value(&retained[0]).unwrap() == evidence,
        "Explicit cancellation and retry must retain the exact reviewed association"
    );
    assert!(
        library.snapshot().await == before,
        "Explicit cancellation and retry must preserve private library and collection state"
    );
    for recording in [&library.recording_id, &library.other_recording_id] {
        assert!(
            matches!(
                hito.downloads.status(&actor, recording).await,
                Err(Error::NotFound)
            ),
            "Profile reads must not create persistent audio downloads"
        );
    }
    let session_hash = format!("{:x}", Sha256::digest(session.token.as_bytes()));
    assert_logs(
        &captured.text(),
        &[
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
            &session.token,
            &session_hash,
            "artist catalog listener password",
            "Bearer ",
            &library.artist_id,
            &library.recording_id,
            &library.source_id,
            &association.id,
        ],
    );
}

fn assert_logs(captured: &str, private_values: &[&str]) {
    for private in private_values {
        assert!(
            !captured.contains(private),
            "Active artist profile logs must omit private values"
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
        "Both explicit profile outcomes must be logged"
    );
    let mut requests = Vec::new();
    for (event, outcome) in events.iter().zip(["cancelled", "available"]) {
        assert!(
            event["fields"]["outcome"] == outcome
                && event["fields"]["elapsed_ms"].as_u64().is_some(),
            "Each explicit profile outcome must include its safe outcome and elapsed time"
        );
        assert!(
            event["span"]["operation"] == "artist_profile",
            "Profile outcomes must retain their operation scope"
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
