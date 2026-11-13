use axum::http::StatusCode;
use hito_core::test_support::source_link_service::{
    Answer, KNOWN_RECORDING, KNOWN_REFERENCE, SourceLinkServer, recording,
};
use hito_core::{
    Actor, AttachReviewedProviderSource, ChooseSource, Credentials, Error, InstallPlugin,
    LookupSourceLink, OfferPluginService, PluginCredential, PluginInstallation, PluginRefresh,
    ProviderCandidate, RefreshPlugin, Services, SetPin,
};
use hito_plugin_sdk::Capability;
use hito_testkit::{held_authority_snapshot::HeldSnapshot, logs::LogCapture};
use serde_json::{Value, json};
use std::{future::pending, time::Duration};
use tracing::instrument::WithSubscriber;
use tracing_subscriber::prelude::*;

const PASSWORD: &str = "indexing test password";

#[path = "source_link_fixture.rs"]
mod fixture;
use fixture::{World, run};

async fn connect(hito: &Services, actor: &Actor, server: &SourceLinkServer) -> PluginInstallation {
    let service = hito
        .plugins
        .offer_service(
            actor,
            OfferPluginService {
                name: format!("Service {}", uuid::Uuid::new_v4()),
                base_url: server.url.clone(),
            },
        )
        .await
        .unwrap();
    hito.plugins
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
        .unwrap()
}

fn request(installation: &PluginInstallation, reference: &str) -> LookupSourceLink {
    LookupSourceLink {
        installation_id: installation.id.clone(),
        installation_revision: installation.revision,
        reference: reference.into(),
    }
}

async fn lookup(
    world: &World,
    actor: &Actor,
    installation: &PluginInstallation,
    reference: &str,
) -> Result<ProviderCandidate, Error> {
    world
        .hito
        .catalog
        .lookup_source_link(
            actor,
            &world.recording,
            request(installation, reference),
            pending(),
        )
        .await
}

async fn snapshot(world: &World) -> Value {
    json!({
        "recordings": world.hito.catalog.recordings(&world.actor, Default::default()).await.unwrap(),
        "sources": world.hito.catalog.sources(&world.actor, &world.recording).await.unwrap(),
        "metadata": world.hito.catalog.recording_metadata(&world.actor, &world.recording, Default::default()).await.unwrap(),
        "shared_sources": world.hito.catalog.sources(&world.actor, &world.shared).await.unwrap(),
    })
}

async fn second_session(world: &World) -> (String, Actor) {
    let session = world
        .hito
        .identity
        .login(Credentials {
            username: "admin".into(),
            password: PASSWORD.into(),
        })
        .await
        .unwrap();
    let actor = world
        .hito
        .identity
        .authenticate(&session.token)
        .await
        .unwrap();
    (session.token, actor)
}

#[tokio::test]
async fn a_pasted_link_returns_an_ordinary_candidate_without_attaching_anything() {
    run("source-link-candidate", async |owned| {
        let server = owned.server().await;
        let world = owned.world.as_ref().unwrap();
        let installation = connect(&world.hito, &world.actor, &server).await;
        let before = snapshot(world).await;

        let candidate = lookup(world, &world.actor, &installation, KNOWN_REFERENCE)
            .await
            .unwrap();

        assert_eq!(
            serde_json::to_value(&candidate).unwrap(),
            json!({
                "installation_id": installation.id,
                "installation_revision": installation.revision,
                "provider_recording_id": KNOWN_RECORDING,
            })
        );
        assert_eq!(
            server.lookup_references(),
            [json!({"reference": KNOWN_REFERENCE})]
        );
        let requests = server.lookup_requests();
        assert!(requests[0].headers.get("cookie").is_none());
        assert!(requests[0].headers.get("authorization").is_none());
        assert_eq!(requests[0].headers["x-hito-plugin-protocol"], "1");
        assert_eq!(server.detail_requests(), 0);
        assert_eq!(server.source_requests(), 0);
        assert_eq!(snapshot(world).await, before);

        let review = world
            .hito
            .catalog
            .review_provider_source(&world.actor, &world.recording, candidate)
            .await
            .unwrap();
        assert_eq!(review.candidate.id, KNOWN_RECORDING);
        assert_eq!(snapshot(world).await, before);
        assert_eq!(server.unexpected(), 0);
    })
    .await;
}

#[tokio::test]
async fn a_link_the_service_does_not_own_is_a_private_not_found_after_authorization() {
    run("source-link-unknown", async |owned| {
        let server = owned.server().await;
        let world = owned.world.as_ref().unwrap();
        let installation = connect(&world.hito, &world.actor, &server).await;

        let result = lookup(
            world,
            &world.actor,
            &installation,
            "https://elsewhere.example/owned",
        )
        .await;
        assert!(
            matches!(result, Err(Error::NotFound)),
            "{:?}",
            result.as_ref().err()
        );
        assert_eq!(server.lookup_references().len(), 1);

        let missing = world
            .hito
            .catalog
            .lookup_source_link(
                &world.actor,
                &uuid::Uuid::new_v4().to_string(),
                request(&installation, KNOWN_REFERENCE),
                pending(),
            )
            .await;
        assert!(
            matches!(missing, Err(Error::NotFound)),
            "{:?}",
            missing.as_ref().err()
        );
        assert_eq!(
            server.lookup_references().len(),
            1,
            "an inaccessible recording must not reach the service"
        );
    })
    .await;
}

#[tokio::test]
async fn a_recording_in_a_shared_library_is_not_found_before_any_service_call() {
    run("source-link-shared", async |owned| {
        let server = owned.server().await;
        let world = owned.world.as_ref().unwrap();
        let installation = connect(&world.hito, &world.actor, &server).await;
        let before = snapshot(world).await;

        let result = world
            .hito
            .catalog
            .lookup_source_link(
                &world.actor,
                &world.shared,
                request(&installation, KNOWN_REFERENCE),
                pending(),
            )
            .await;

        assert!(
            matches!(result, Err(Error::NotFound)),
            "{:?}",
            result.as_ref().err()
        );
        assert!(server.lookup_references().is_empty());
        assert_eq!(snapshot(world).await, before);
    })
    .await;
}

#[tokio::test]
async fn invalid_references_are_rejected_before_any_service_call() {
    run("source-link-invalid", async |owned| {
        let server = owned.server().await;
        let world = owned.world.as_ref().unwrap();
        let installation = connect(&world.hito, &world.actor, &server).await;
        for reference in [
            String::new(),
            "   ".into(),
            "a".repeat(2049),
            "owned\nlink".into(),
            "owned\u{7f}link".into(),
        ] {
            let result = lookup(world, &world.actor, &installation, &reference).await;
            assert!(
                matches!(result, Err(Error::Invalid(_))),
                "{:?}",
                result.as_ref().err()
            );
        }
        assert!(server.lookup_references().is_empty());
    })
    .await;
}

#[tokio::test]
async fn an_old_approval_never_grants_source_links_until_the_owner_accepts_the_change() {
    run("source-link-approval", async |owned| {
        let server = owned.server_with_capabilities(
            "org.hito.upgraded-source-link-fixture",
            [Capability::Search, Capability::Sources],
        )
        .await;
        let world = owned.world.as_ref().unwrap();
        let installation = connect(&world.hito, &world.actor, &server).await;
        let refused = lookup(world, &world.actor, &installation, KNOWN_REFERENCE).await;
        assert!(
            matches!(refused, Err(Error::Invalid(_))),
            "{:?}",
            refused.as_ref().err()
        );
        assert!(
            server.lookup_references().is_empty(),
            "an approval without source_links must not reach the service"
        );

        server.declare([
            Capability::Search,
            Capability::Sources,
            Capability::SourceLinks,
        ]);
        let review = world
            .hito
            .plugins
            .refresh(
                &world.actor,
                &installation.id,
                RefreshPlugin {
                    revision: installation.revision,
                    accept_manifest_digest: None,
                    credential: PluginCredential::Keep,
                },
            )
            .await
            .unwrap();
        let PluginRefresh::ReviewRequired { digest, .. } = review else {
            panic!("a new capability must require review");
        };
        let pending_review = lookup(world, &world.actor, &installation, KNOWN_REFERENCE).await;
        assert!(
            matches!(pending_review, Err(Error::Invalid(_))),
            "{:?}",
            pending_review.as_ref().err()
        );
        assert!(server.lookup_references().is_empty());

        let accepted = world
            .hito
            .plugins
            .refresh(
                &world.actor,
                &installation.id,
                RefreshPlugin {
                    revision: installation.revision,
                    accept_manifest_digest: Some(digest),
                    credential: PluginCredential::Keep,
                },
            )
            .await
            .unwrap();
        let PluginRefresh::Updated {
            installation: upgraded,
        } = accepted
        else {
            panic!("an accepted declaration must update the connection");
        };
        let stale = lookup(world, &world.actor, &installation, KNOWN_REFERENCE).await;
        assert!(
            matches!(stale, Err(Error::Conflict(_))),
            "{:?}",
            stale.as_ref().err()
        );
        assert!(server.lookup_references().is_empty());

        let candidate = lookup(world, &world.actor, &upgraded, KNOWN_REFERENCE)
            .await
            .unwrap();
        assert_eq!(candidate.installation_revision, upgraded.revision);
        assert_eq!(server.lookup_references().len(), 1);
    })
    .await;
}

#[tokio::test]
async fn the_reference_reaches_only_the_selected_service() {
    run("source-link-selected", async |owned| {
        let selected = owned.server().await;
        let other = owned.server().await;
        let world = owned.world.as_ref().unwrap();
        let installation = connect(&world.hito, &world.actor, &selected).await;
        connect(&world.hito, &world.actor, &other).await;

        lookup(world, &world.actor, &installation, KNOWN_REFERENCE)
            .await
            .unwrap();
        assert_eq!(selected.lookup_references().len(), 1);
        assert!(other.lookup_references().is_empty());
        assert_eq!(other.detail_requests(), 0);
    })
    .await;
}

#[derive(Clone, Copy, Debug)]
enum Revocation {
    Logout,
    PinLock,
    RemoveConnection,
    ChangeConnectionRevision,
}

async fn revoke_during_lookup(revocation: Revocation) {
    run("source-link-revocation", async |owned| {
        let server = owned.server().await;
        let world = owned.world.as_ref().unwrap();
        let installation = connect(&world.hito, &world.actor, &server).await;
        let (token, actor) = second_session(world).await;
        if matches!(revocation, Revocation::PinLock) {
            world
                .hito
                .identity
                .set_pin(
                    &actor,
                    SetPin {
                        password: PASSWORD.into(),
                        pin: Some("1234".into()),
                        idle_minutes: 0,
                    },
                )
                .await
                .unwrap();
        }
        let before = snapshot(world).await;
        let gate = server.hold_lookup();
        owned.gates.push(gate.clone());
        {
            let catalog = world.hito.catalog.clone();
            let recording = world.recording.clone();
            let request = request(&installation, KNOWN_REFERENCE);
            let actor = actor.clone();
            owned.requests.spawn(async move {
                catalog
                    .lookup_source_link(&actor, &recording, request, pending())
                    .await
            })
        };
        gate.entered().await;
        match revocation {
            Revocation::Logout => world.hito.identity.logout(&token).await.unwrap(),
            Revocation::PinLock => {
                world.hito.identity.lock(&actor).await.unwrap();
            }
            Revocation::RemoveConnection => world
                .hito
                .plugins
                .remove(&world.actor, &installation.id, installation.revision)
                .await
                .unwrap(),
            Revocation::ChangeConnectionRevision => {
                let changed = world
                    .hito
                    .plugins
                    .clear_credential(&world.actor, &installation.id, installation.revision)
                    .await
                    .unwrap();
                assert_eq!(changed.revision, installation.revision + 1);
            }
        }
        gate.open();
        let result = tokio::time::timeout(Duration::from_secs(3), async { owned.requests.join_next().await.unwrap() })
            .await
            .expect("revocation must finish the lookup promptly")
            .unwrap();
        match revocation {
            Revocation::Logout => assert!(
                matches!(result, Err(Error::Unauthorized)),
                "{:?}",
                result.as_ref().err()
            ),
            Revocation::PinLock => assert!(
                matches!(result, Err(Error::Locked)),
                "{:?}",
                result.as_ref().err()
            ),
            Revocation::RemoveConnection => {
                assert!(
                    matches!(result, Err(Error::NotFound)),
                    "{:?}",
                    result.as_ref().err()
                )
            }
            Revocation::ChangeConnectionRevision => {
                assert!(
                    matches!(result, Err(Error::Conflict(_))),
                    "{:?}",
                    result.as_ref().err()
                )
            }
        }
        assert_eq!(server.lookup_references().len(), 1);
        assert_eq!(server.detail_requests(), 0);
        assert_eq!(snapshot(world).await, before);
    })
    .await;
}

#[tokio::test]
async fn logout_during_the_lookup_publishes_no_candidate() {
    revoke_during_lookup(Revocation::Logout).await;
}

#[tokio::test]
async fn a_pin_lock_during_the_lookup_publishes_no_candidate() {
    revoke_during_lookup(Revocation::PinLock).await;
}

#[tokio::test]
async fn removing_the_connection_during_the_lookup_publishes_no_candidate() {
    revoke_during_lookup(Revocation::RemoveConnection).await;
}

#[tokio::test]
async fn a_connection_revision_change_during_the_lookup_publishes_no_candidate() {
    revoke_during_lookup(Revocation::ChangeConnectionRevision).await;
}

#[tokio::test]
async fn a_not_found_answer_after_revocation_reports_the_revocation() {
    run("source-link-revoked-not-found", async |owned| {
        let server = owned.server().await;
        let world = owned.world.as_ref().unwrap();
        let installation = connect(&world.hito, &world.actor, &server).await;
        let (token, actor) = second_session(world).await;
        let gate = server.hold_lookup();
        owned.gates.push(gate.clone());
        {
            let catalog = world.hito.catalog.clone();
            let recording = world.recording.clone();
            let request = request(&installation, "https://elsewhere.example/owned");
            owned.requests.spawn(async move {
                catalog
                    .lookup_source_link(&actor, &recording, request, pending())
                    .await
            })
        };
        gate.entered().await;
        world.hito.identity.logout(&token).await.unwrap();
        gate.open();
        let result = tokio::time::timeout(Duration::from_secs(3), async { owned.requests.join_next().await.unwrap() })
            .await
            .unwrap()
            .unwrap();
        assert!(
            matches!(result, Err(Error::Unauthorized)),
            "{:?}",
            result.as_ref().err()
        );
    })
    .await;
}

#[tokio::test]
async fn cancelling_the_lookup_finishes_promptly_without_a_candidate() {
    run("source-link-cancel", async |owned| {
        let server = owned.server().await;
        let world = owned.world.as_ref().unwrap();
        let installation = connect(&world.hito, &world.actor, &server).await;
        let before = snapshot(world).await;
        let gate = server.hold_lookup();
        owned.gates.push(gate.clone());
        let (cancel, cancelled) = tokio::sync::oneshot::channel::<()>();
        {
            let catalog = world.hito.catalog.clone();
            let recording = world.recording.clone();
            let request = request(&installation, KNOWN_REFERENCE);
            let actor = world.actor.clone();
            owned.requests.spawn(async move {
                catalog
                    .lookup_source_link(&actor, &recording, request, async {
                        let _ = cancelled.await;
                    })
                    .await
            })
        };
        gate.entered().await;
        cancel.send(()).unwrap();
        let result = tokio::time::timeout(Duration::from_secs(3), async { owned.requests.join_next().await.unwrap() })
            .await
            .expect("cancellation must finish the lookup promptly")
            .unwrap();
        assert!(
            matches!(result, Err(Error::Upstream(_))),
            "{:?}",
            result.as_ref().err()
        );
        assert_eq!(snapshot(world).await, before);
        assert_eq!(server.detail_requests(), 0);
    })
    .await;
}

#[tokio::test]
async fn a_stalled_answer_body_is_retired_on_cancellation() {
    run("source-link-stalled-body", async |owned| {
        let server = owned.server().await;
        let world = owned.world.as_ref().unwrap();
        let installation = connect(&world.hito, &world.actor, &server).await;
        let gate = server.stall_lookup_body(8);
        owned.gates.push(gate.clone());
        let (cancel, cancelled) = tokio::sync::oneshot::channel::<()>();
        {
            let catalog = world.hito.catalog.clone();
            let recording = world.recording.clone();
            let request = request(&installation, KNOWN_REFERENCE);
            let actor = world.actor.clone();
            owned.requests.spawn(async move {
                catalog
                    .lookup_source_link(&actor, &recording, request, async {
                        let _ = cancelled.await;
                    })
                    .await
            })
        };
        gate.entered().await;
        cancel.send(()).unwrap();
        let result = tokio::time::timeout(Duration::from_secs(3), async { owned.requests.join_next().await.unwrap() })
            .await
            .unwrap()
            .unwrap();
        assert!(
            matches!(result, Err(Error::Upstream(_))),
            "{:?}",
            result.as_ref().err()
        );
        gate.retired().await;
    })
    .await;
}

#[tokio::test]
async fn upstream_failures_are_sanitized_and_keep_the_listener_session() {
    run("source-link-upstream", async |owned| {
        let server = owned.server().await;
        let world = owned.world.as_ref().unwrap();
        let installation = connect(&world.hito, &world.actor, &server).await;
        for (reference, answer) in [
            (
                "owned-unauthorized",
                Answer::Status(StatusCode::UNAUTHORIZED),
            ),
            ("owned-broken", Answer::Status(StatusCode::BAD_GATEWAY)),
            (
                "owned-invalid",
                Answer::Raw(json!({"recording": recording("bad\nid")})),
            ),
            (
                "owned-echo",
                Answer::Raw(json!({"recording": recording("owned-echo"), "reference": "owned-echo"})),
            ),
        ] {
            server.answer(reference, answer);
            let result = lookup(world, &world.actor, &installation, reference).await;
            if reference == "owned-unauthorized" {
                match result {
                    Err(Error::PluginCredentialsRequired(Some(stamp))) => {
                        assert_eq!(stamp.installation_id, installation.id);
                        assert_eq!(stamp.installation_revision, installation.revision);
                    }
                    other => panic!("expected a stamped credential refusal, got {:?}", other.err()),
                }
                continue;
            }
            match result {
                Err(Error::Upstream(message)) => assert!(!message.contains(reference)),
                other => panic!("expected a sanitized upstream error, got {:?}", other.err()),
            }
        }
        world
            .hito
            .identity
            .check_session(&world.actor)
            .await
            .unwrap();
    })
    .await;
}

#[tokio::test]
async fn lookup_logs_carry_safe_outcomes_and_never_the_reference() {
    run("source-link-logs", async |owned| {
        let server = owned.server().await;
        let world = owned.world.as_ref().unwrap();
        let installation = connect(&world.hito, &world.actor, &server).await;
        let private = "https://links.example/owned/private-link-sentinel?token=private-token-sentinel";
        server.answer(
            private,
            Answer::Recording(recording("private-recording-sentinel")),
        );
        let capture = LogCapture::default();
        let dispatch = capture.dispatch_with_spans("hito_core=trace");
        async {
            lookup(world, &world.actor, &installation, private)
                .await
                .unwrap();
            let missing = lookup(
                world,
                &world.actor,
                &installation,
                "https://links.example/owned/private-missing-sentinel",
            )
            .await;
            assert!(matches!(missing, Err(Error::NotFound)));
        }
        .with_subscriber(dispatch)
        .await;
        let events = capture.events();
        let outcomes: Vec<_> = events
            .iter()
            .filter(|event| event["fields"]["event"] == "source_link_lookup_finished")
            .map(|event| event["fields"]["outcome"].clone())
            .collect();
        assert_eq!(outcomes, [json!("found"), json!("not_found")]);
        capture.assert_excludes(&[
            "private-link-sentinel",
            "private-token-sentinel",
            "private-missing-sentinel",
            "private-recording-sentinel",
            "links.example",
        ]);
    })
    .await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_source_choice_made_during_the_connection_recheck_publishes_no_candidate() {
    run("source-link-choice-during-recheck", async |owned| {
        let server = owned.server().await;
        let world = owned.world.as_ref().unwrap();
        let installation = connect(&world.hito, &world.actor, &server).await;
        let candidate = lookup(world, &world.actor, &installation, KNOWN_REFERENCE)
            .await
            .unwrap();
        let review = world
            .hito
            .catalog
            .review_provider_source(&world.actor, &world.recording, candidate.clone())
            .await
            .unwrap();
        world
            .hito
            .catalog
            .attach_provider_source(
                &world.actor,
                &world.recording,
                AttachReviewedProviderSource {
                    candidate,
                    review_stamp: review.review_stamp,
                },
            )
            .await
            .unwrap();
        let attached = world
            .hito
            .catalog
            .sources(&world.actor, &world.recording)
            .await
            .unwrap()
            .remove(0);
        let lookups = server.lookup_references().len();
        let held = HeldSnapshot::at("source_link_postcheck");
        {
            let catalog = world.hito.catalog.clone();
            let recording = world.recording.clone();
            let request = request(&installation, KNOWN_REFERENCE);
            let actor = world.actor.clone();
            owned.requests.spawn(
                async move {
                    catalog
                        .lookup_source_link(&actor, &recording, request, pending())
                        .await
                }
                .with_subscriber(tracing_subscriber::registry().with(held.clone())),
            );
        }
        held.entered().await;
        let chosen = world
            .hito
            .catalog
            .choose_source(
                &world.actor,
                &world.recording,
                ChooseSource {
                    request_id: uuid::Uuid::new_v4(),
                    source_id: uuid::Uuid::parse_str(&attached.id).unwrap(),
                },
            )
            .await;
        held.release();
        chosen.unwrap();
        let result = tokio::time::timeout(Duration::from_secs(3), async {
            owned.requests.join_next().await.unwrap()
        })
        .await
        .expect("the held lookup must finish once released")
        .unwrap();
        held.assert_returned();
        assert!(
            matches!(result, Err(Error::Conflict(_))),
            "{:?}",
            result
                .as_ref()
                .map(|candidate| &candidate.provider_recording_id)
        );
        assert_eq!(server.lookup_references().len(), lookups + 1);
    })
    .await;
}
