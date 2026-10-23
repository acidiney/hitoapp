#[path = "fixtures/artist_catalog_profiles_library.rs"]
#[allow(dead_code)]
mod library;
use hito_core::test_support::artist_catalog_editions_service as provider;
use hito_core::test_support::metadata_tag_logs as logs;
use hito_core::test_support::support;

use axum::http::Method;
use futures_util::FutureExt;
use hito_core::{
    Actor, Credentials, EditionTrackListAvailability, Error, ReadEditionTrackList,
    ReadReleaseGroupEditions, ReleaseGroupEditionsAvailability, UpdateMetadataSettings,
};
use hito_plugin_sdk::CatalogEditionOperation;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    collections::HashSet,
    panic::{AssertUnwindSafe, resume_unwind},
    sync::Arc,
    time::Duration,
};
use tokio::{sync::oneshot, task::JoinSet, time::timeout};
use tracing::instrument::WithSubscriber;
use uuid::Uuid;

#[tokio::test]
async fn held_edition_reads_recheck_policy_and_release_capacity_after_cancellation() {
    let library = library::Library::prepare().await;
    let mut server = provider::CatalogServer::start().await;
    let mut reads = JoinSet::new();
    let mut gates = Vec::new();
    let result = AssertUnwindSafe(timeout(
        Duration::from_secs(30),
        exercise(&library, &server, &mut reads, &mut gates),
    ))
    .catch_unwind()
    .await;
    for gate in gates {
        gate.release();
    }
    reads.shutdown().await;
    let joined = server.finish().await;
    let result = match result {
        Ok(result) => result,
        Err(panic) => resume_unwind(panic),
    };
    assert!(joined, "The owned edition HTTP server must join shutdown");
    result.expect("The held edition workflow must complete within its bound");
}

async fn exercise(
    library: &library::Library,
    server: &provider::CatalogServer,
    reads: &mut JoinSet<()>,
    gates: &mut Vec<Arc<provider::EditionGate>>,
) {
    let hito = &library.hito;
    let association = library
        .review_artist(&server.url, provider::ARTIST_ID)
        .await;
    server.enable_edition_listing();
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
    let events_before =
        serde_json::to_value(hito.collection.events(&actor, 0).await.unwrap()).unwrap();
    assert_no_downloads(library, &actor).await;
    let captured = logs::CapturedLogs::default();
    let writer = captured.clone();
    let dispatch = tracing::Dispatch::new(
        tracing_subscriber::fmt()
            .with_env_filter("off,hito_core=trace")
            .with_ansi(false)
            .without_time()
            .json()
            .with_span_events(tracing_subscriber::fmt::format::FmtSpan::CLOSE)
            .with_writer(move || writer.clone())
            .finish(),
    );

    let listing_gate = server.hold(CatalogEditionOperation::ReleaseGroupEditions);
    gates.push(listing_gate.clone());
    let catalog = hito.catalog.clone();
    let request_actor = actor.clone();
    let recording = library.recording_id.clone();
    let association_id = association.id.clone();
    reads.spawn(
        async move {
            let result = catalog
                .recording_artist_release_group_editions_until(
                    &request_actor,
                    &recording,
                    &association_id,
                    listing(None),
                    std::future::pending(),
                )
                .await;
            assert!(
                matches!(result, Err(Error::Conflict(_))),
                "A held response must not return metadata under an earlier policy revision"
            );
        }
        .with_subscriber(dispatch.clone()),
    );
    timeout(Duration::from_secs(3), listing_gate.entered())
        .await
        .expect("The edition list must reach the owned provider before policy changes");
    let admitted = server.requests();
    let settings = hito.catalog.metadata_settings(&actor).await.unwrap();
    let revised = timeout(
        Duration::from_secs(3),
        hito.catalog.set_metadata_settings(
            &actor,
            UpdateMetadataSettings {
                revision: settings.revision,
                enabled: true,
                providers: settings.providers,
            },
        ),
    )
    .await
    .expect("Policy changes must complete while the provider response remains held")
    .unwrap();
    assert!(revised.revision > settings.revision);
    assert_eq!(
        server.requests(),
        admitted,
        "Policy changes must not retry provider I/O"
    );
    listing_gate.release();
    timeout(Duration::from_secs(3), reads.join_next())
        .await
        .expect("The stale read must finish after provider release")
        .unwrap()
        .unwrap();
    timeout(Duration::from_secs(3), listing_gate.retired())
        .await
        .expect("The stale list handler must retire");
    assert_eq!(server.requests(), admitted);

    let first = timeout(
        Duration::from_secs(3),
        read_editions(library, &actor, &association.id, None).with_subscriber(dispatch.clone()),
    )
    .await
    .expect("A manual retry must be admitted after the stale listing finishes")
    .unwrap();
    let ReleaseGroupEditionsAvailability::Available {
        association_id,
        page: first_page,
        continuation: Some(continuation),
    } = first
    else {
        panic!("Current policy must permit a fresh first edition page");
    };
    assert_eq!(association_id, association.id);
    assert!(first_page == provider::expected_editions(false));
    let second = timeout(
        Duration::from_secs(3),
        read_editions(library, &actor, &association.id, Some(continuation))
            .with_subscriber(dispatch.clone()),
    )
    .await
    .expect("The explicit next page must complete")
    .unwrap();
    let ReleaseGroupEditionsAvailability::Available {
        association_id,
        page: second_page,
        continuation: None,
    } = second
    else {
        panic!("The chosen group's final edition page must be available");
    };
    assert_eq!(association_id, association.id);
    assert!(second_page == provider::expected_editions(true));
    let selected = second_page
        .items
        .iter()
        .find(|edition| edition.id == provider::BONUS_ID)
        .unwrap();
    assert!(
        server
            .requests()
            .iter()
            .all(|request| request.path != "/prefix/v1/catalog/editions/track-list"),
        "Listing must not eagerly read a track list"
    );

    let track_gate = server.hold(CatalogEditionOperation::EditionTrackList);
    gates.push(track_gate.clone());
    let (cancel, cancelled) = oneshot::channel();
    let catalog = hito.catalog.clone();
    let request_actor = actor.clone();
    let recording = library.recording_id.clone();
    let association_id = association.id.clone();
    let selected_id = selected.id.clone();
    reads.spawn(
        async move {
            let result = catalog
                .recording_artist_edition_track_list_until(
                    &request_actor,
                    &recording,
                    &association_id,
                    ReadEditionTrackList {
                        release_group_id: provider::GROUP_ID.into(),
                        edition_id: selected_id,
                    },
                    async {
                        let _ = cancelled.await;
                    },
                )
                .await;
            assert!(
                matches!(result, Ok(EditionTrackListAvailability::Cancelled)),
                "Explicit cancellation must finish without provider metadata"
            );
        }
        .with_subscriber(dispatch.clone()),
    );
    timeout(Duration::from_secs(3), track_gate.entered())
        .await
        .expect("The selected track request must reach the owned provider before cancellation");
    let admitted = server.requests();
    cancel
        .send(())
        .expect("The explicit cancellation receiver must remain owned");
    timeout(Duration::from_secs(3), reads.join_next())
        .await
        .expect("Cancellation must finish while the original provider handler remains held")
        .unwrap()
        .unwrap();
    assert_eq!(
        server.requests(),
        admitted,
        "Cancellation must not retry provider I/O"
    );
    assert!(
        !track_gate.has_retired(),
        "The original handler must still be held before the manual retry"
    );
    let retry = timeout(
        Duration::from_secs(3),
        hito.catalog
            .recording_artist_edition_track_list_until(
                &actor,
                &library.recording_id,
                &association.id,
                ReadEditionTrackList {
                    release_group_id: provider::GROUP_ID.into(),
                    edition_id: selected.id.clone(),
                },
                std::future::pending(),
            )
            .with_subscriber(dispatch),
    )
    .await
    .expect(
        "Cancellation must release Hito account capacity before the original handler is released",
    )
    .unwrap();
    let EditionTrackListAvailability::Available {
        association_id,
        edition,
    } = retry
    else {
        panic!("The explicit retry must return the selected bonus edition");
    };
    assert_eq!(association_id, association.id);
    assert!(edition == provider::expected_track_list());
    assert!(
        !track_gate.has_retired(),
        "The exact metadata retry must finish while the original handler remains held"
    );
    track_gate.release();
    timeout(Duration::from_secs(3), track_gate.retired())
        .await
        .expect("The original track handler must retire after its explicit release");

    let requests = server.requests();
    let pages = requests
        .iter()
        .filter(|request| request.path == "/prefix/v1/catalog/release-groups/editions")
        .collect::<Vec<_>>();
    assert_eq!(pages.len(), 3);
    for (request, cursor) in
        pages
            .iter()
            .zip([Value::Null, Value::Null, json!(provider::PROVIDER_CURSOR)])
    {
        assert!(request.method == Method::POST);
        assert_eq!(
            request.body,
            json!({"release_group_id": provider::GROUP_ID, "cursor": cursor, "snapshot": null, "limit": 1})
        );
    }
    let tracks = requests
        .iter()
        .filter(|request| request.path == "/prefix/v1/catalog/editions/track-list")
        .collect::<Vec<_>>();
    assert_eq!(tracks.len(), 2);
    for request in tracks {
        assert!(request.method == Method::POST);
        assert_eq!(
            request.body,
            json!({"release_group_id": provider::GROUP_ID, "edition_id": selected.id})
        );
    }
    assert!(library.snapshot().await == before);
    assert_eq!(
        serde_json::to_value(hito.collection.events(&actor, 0).await.unwrap()).unwrap(),
        events_before
    );
    let retained = hito
        .catalog
        .recording_artist_associations(&actor, &library.recording_id)
        .await
        .unwrap();
    assert!(retained.len() == 1 && serde_json::to_value(&retained[0]).unwrap() == evidence);
    assert!(
        hito.catalog
            .recording_artist_associations(&actor, &library.other_recording_id)
            .await
            .unwrap()
            .is_empty()
    );
    assert_no_downloads(library, &actor).await;
    assert_eq!(server.unexpected(), 0);
    assert!(
        requests.iter().all(|request| matches!(
            request.path.as_str(),
            "/prefix/v1/manifest"
                | "/prefix/v1/catalog/capabilities"
                | "/prefix/v1/catalog/artists/profile"
                | "/prefix/v1/catalog/artists/discography"
                | "/prefix/v1/catalog/editions/capabilities"
                | "/prefix/v1/catalog/release-groups/editions"
                | "/prefix/v1/catalog/editions/track-list"
        )),
        "Edition reads must not request sources, audio, downloads or adoption"
    );

    let session_hash = format!("{:x}", Sha256::digest(session.token.as_bytes()));
    let mut private = vec![
        provider::ARTIST_ID,
        provider::GROUP_ID,
        provider::BONUS_ID,
        provider::PROVIDER_CURSOR,
        "org.hito.artist-editions-fixture",
        "Owned performer",
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
        &library.other_recording_id,
        &library.source_id,
        &library.observation_id,
        &association.id,
        &association.library_id,
    ];
    if let hito_core::MetadataProvider::HttpPlugin { installation_id } = &association.provider {
        private.push(installation_id);
    }
    for page in [&first_page, &second_page] {
        for item in &page.items {
            private.push(&item.id);
            private.push(&item.title);
            if let Some(disambiguation) = &item.disambiguation {
                private.push(disambiguation);
            }
        }
    }
    for medium in &edition.media {
        if let Some(title) = &medium.title {
            private.push(title);
        }
        for track in &medium.tracks {
            private.push(&track.id);
            private.push(&track.title);
            if let Some(recording) = &track.recording_id {
                private.push(recording);
            }
        }
    }
    assert_logs(&captured.text(), &private);
}

fn listing(continuation: Option<String>) -> ReadReleaseGroupEditions {
    ReadReleaseGroupEditions {
        release_group_id: provider::GROUP_ID.into(),
        limit: 1,
        continuation,
    }
}

async fn read_editions(
    library: &library::Library,
    actor: &Actor,
    association: &str,
    continuation: Option<String>,
) -> hito_core::Result<ReleaseGroupEditionsAvailability> {
    library
        .hito
        .catalog
        .recording_artist_release_group_editions_until(
            actor,
            &library.recording_id,
            association,
            listing(continuation),
            std::future::pending(),
        )
        .await
}

async fn assert_no_downloads(library: &library::Library, actor: &Actor) {
    for recording in [&library.recording_id, &library.other_recording_id] {
        assert!(
            matches!(
                library.hito.downloads.status(actor, recording).await,
                Err(Error::NotFound)
            ),
            "Edition reads must preserve explicit-download absence"
        );
    }
}

fn assert_logs(captured: &str, private: &[&str]) {
    for value in private {
        assert!(
            !captured.contains(value),
            "Edition workflow logs must omit private values"
        );
    }
    let records = captured
        .lines()
        .map(|line| {
            serde_json::from_str::<Value>(line)
                .expect("Edition workflow logs must use structured JSON")
        })
        .collect::<Vec<_>>();
    let events = records
        .iter()
        .filter(|event| {
            matches!(
                event["fields"]["message"].as_str(),
                Some(
                    "Artist release-group editions read completed"
                        | "Artist edition track list read completed"
                )
            )
        })
        .collect::<Vec<_>>();
    assert_eq!(
        events.len(),
        5,
        "Each explicit read must log exactly one completion"
    );
    let mut requests = HashSet::new();
    for (event, outcome) in events.iter().zip([
        "conflict",
        "available",
        "available",
        "cancelled",
        "available",
    ]) {
        assert_eq!(event["fields"]["outcome"], outcome);
        assert!(event["fields"]["elapsed_ms"].as_u64().is_some());
        let request = event["span"]["request_id"]
            .as_str()
            .expect("Every completion needs a correlation ID");
        assert!(
            Uuid::parse_str(request).is_ok() && requests.insert(request),
            "Separate attempts need distinct generated correlation IDs"
        );
    }
    for (event, count) in events[..3].iter().zip([0, 1, 1]) {
        assert_eq!(event["span"]["operation"], "artist_release_group_editions");
        assert_eq!(event["fields"]["item_count"], count);
    }
    for (event, (media, tracks)) in events[3..].iter().zip([(0, 0), (2, 4)]) {
        assert_eq!(event["span"]["operation"], "artist_edition_track_list");
        assert_eq!(event["fields"]["medium_count"], media);
        assert_eq!(event["fields"]["track_count"], tracks);
    }
    let closed = records
        .iter()
        .filter(|event| {
            event["fields"]["message"] == "close"
                && matches!(
                    event["span"]["operation"].as_str(),
                    Some("artist_release_group_editions" | "artist_edition_track_list")
                )
        })
        .map(|event| {
            event["span"]["request_id"]
                .as_str()
                .expect("Closed edition spans need their correlation ID")
        })
        .collect::<Vec<_>>();
    assert_eq!(
        closed.len(),
        5,
        "Every edition workflow span must be captured at close"
    );
    assert!(
        closed.into_iter().collect::<HashSet<_>>() == requests,
        "Span closes must retain the five completion correlations"
    );
}
