use hito_core::test_support::artist_identity_selection_catalog as catalog;
#[path = "fixtures/artist_identity_selection_library.rs"]
mod library;
use hito_core::test_support::metadata_tag_logs as logs;
use hito_core::test_support::support;

use futures_util::FutureExt;
use hito_core::{
    ArtistIdentityGroupQuery, ChooseRecordingArtistIdentity, Credentials, Error, MetadataProvider,
    ResetRecordingArtistIdentity,
};
use serde_json::Value;
use std::panic::{AssertUnwindSafe, resume_unwind};
use tracing::instrument::WithSubscriber;
use uuid::Uuid;

#[tokio::test]
async fn identity_workflows_log_correlated_outcomes_and_timings_without_private_context() {
    let mut library = library::Library::prepare().await;
    let result = AssertUnwindSafe(exercise(&library)).catch_unwind().await;
    library.origin.shutdown().await;
    library.catalog.finish().await;
    if let Err(panic) = result {
        resume_unwind(panic);
    }
}

async fn exercise(library: &library::Library) {
    let hito = &library.hito;
    let session = hito
        .identity
        .login(Credentials {
            username: "admin".into(),
            password: "indexing test password".into(),
        })
        .await
        .unwrap();
    let actor = hito.identity.authenticate(&session.token).await.unwrap();
    let recording = &library.local;
    let context = hito
        .catalog
        .begin_artist_review(&actor, recording, library.metadata_provider.clone())
        .await
        .unwrap();
    let association = hito
        .catalog
        .confirm_artist_review(&actor, &context, catalog::ALPHA)
        .await
        .unwrap();
    let choose_id = Uuid::new_v4();
    let reset_id = Uuid::new_v4();
    let missing_recording = Uuid::new_v4().to_string();
    let before = library.snapshot().await;
    let requests = library.catalog.requests();
    let origin = library.origin_requests().await;
    let captured = logs::CapturedLogs::default();
    let dispatch = tracing::Dispatch::new(captured.subscriber());
    let group = async {
        let chosen = hito
            .catalog
            .choose_recording_artist_identity(
                &actor,
                recording,
                ChooseRecordingArtistIdentity {
                    request_id: choose_id,
                    expected_revision: 0,
                    association_id: Uuid::parse_str(&association.id).unwrap(),
                    context: context.clone(),
                },
            )
            .await
            .unwrap();
        let group = chosen.selection.unwrap().group_id;
        hito.catalog
            .artist_identity_group(&actor, &group, Default::default())
            .await
            .unwrap();
        assert!(matches!(
            hito.catalog
                .artist_identity_group(
                    &actor,
                    "private-artist-group-route-canary",
                    Default::default()
                )
                .await,
            Err(Error::NotFound)
        ));
        hito.catalog
            .artist_identity_groups(
                &actor,
                ArtistIdentityGroupQuery {
                    q: Some("private-query-title-canary ' OR 1=1".into()),
                    ..Default::default()
                },
            )
            .await
            .unwrap();
        hito.catalog
            .reset_recording_artist_identity(
                &actor,
                recording,
                ResetRecordingArtistIdentity {
                    request_id: reset_id,
                    expected_revision: 1,
                },
            )
            .await
            .unwrap();
        hito.catalog
            .choose_recording_artist_identity(
                &actor,
                recording,
                ChooseRecordingArtistIdentity {
                    request_id: choose_id,
                    expected_revision: 0,
                    association_id: Uuid::parse_str(&association.id).unwrap(),
                    context: context.clone(),
                },
            )
            .await
            .unwrap();
        assert!(matches!(
            hito.catalog
                .choose_recording_artist_identity(
                    &actor,
                    recording,
                    ChooseRecordingArtistIdentity {
                        request_id: choose_id,
                        expected_revision: 2,
                        association_id: Uuid::parse_str(&association.id).unwrap(),
                        context: context.clone(),
                    }
                )
                .await,
            Err(Error::Conflict(_))
        ));
        hito.catalog
            .reset_recording_artist_identity(
                &actor,
                recording,
                ResetRecordingArtistIdentity {
                    request_id: reset_id,
                    expected_revision: 1,
                },
            )
            .await
            .unwrap();
        hito.catalog
            .recording_artist_identity_choice(&actor, recording)
            .await
            .unwrap();
        assert!(matches!(
            hito.catalog
                .recording_artist_identity_choice(&actor, &missing_recording)
                .await,
            Err(Error::NotFound)
        ));
        hito.catalog
            .artist_identity_groups(&actor, Default::default())
            .await
            .unwrap();
        group
    }
    .with_subscriber(dispatch)
    .await;
    let text = captured.text();
    let MetadataProvider::HttpPlugin { installation_id } = &library.metadata_provider else {
        unreachable!()
    };
    for private in [
        recording.as_str(),
        library.imported.as_str(),
        library.namesake.as_str(),
        context.observation_id.as_str(),
        association.id.as_str(),
        group.as_str(),
        installation_id.as_str(),
        session.token.as_str(),
        "indexing test password",
        catalog::CREDIT,
        catalog::ALPHA,
        "Owned local recording",
        "Private imported title",
        library.catalog.url.as_str(),
        "private-query-title-canary",
        "private-artist-group-route-canary",
        missing_recording.as_str(),
    ] {
        assert!(
            !text.contains(private),
            "Identity workflow logs must omit private context"
        );
    }
    if let Some(source) = context.source_id {
        assert!(!text.contains(&source));
    }
    let events = text
        .lines()
        .map(|line| serde_json::from_str::<Value>(line).unwrap())
        .collect::<Vec<_>>();
    for (message, operation, outcomes) in [
        (
            "Artist identity choice finished",
            "artist_identity_choose",
            vec!["accepted", "accepted", "conflict"],
        ),
        (
            "Artist identity reset finished",
            "artist_identity_reset",
            vec!["accepted", "accepted"],
        ),
        (
            "Artist identity choice loaded",
            "artist_identity_choice_read",
            vec!["success", "not_found"],
        ),
        (
            "Artist identity groups loaded",
            "artist_identity_groups",
            vec!["success", "success"],
        ),
        (
            "Artist identity group loaded",
            "artist_identity_group",
            vec!["success", "not_found"],
        ),
    ] {
        let completed = events
            .iter()
            .filter(|event| event["fields"]["message"] == message)
            .collect::<Vec<_>>();
        assert_eq!(
            completed.len(),
            outcomes.len(),
            "Every successful or failed public workflow must have a completion event: {operation}"
        );
        for (event, outcome) in completed.iter().zip(outcomes) {
            assert_eq!(event["fields"]["outcome"], outcome);
            assert!(event["fields"]["elapsed_ms"].as_u64().is_some());
            assert_eq!(event["span"]["operation"], operation);
            assert!(Uuid::parse_str(event["span"]["request_id"].as_str().unwrap()).is_ok());
            if operation == "artist_identity_choose" {
                assert_eq!(event["span"]["request_id"], choose_id.to_string());
            }
            if operation == "artist_identity_reset" {
                assert_eq!(event["span"]["request_id"], reset_id.to_string());
            }
        }
    }
    assert_eq!(
        events
            .iter()
            .filter(|event| event["fields"]["outcome"] == "replayed")
            .count(),
        2
    );
    assert_eq!(library.snapshot().await, before);
    assert_eq!(library.catalog.requests(), requests);
    assert_eq!(library.origin_requests().await, origin);
}
