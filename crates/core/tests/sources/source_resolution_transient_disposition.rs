use crate::resolution_musical_fixture as saved;
use futures_util::FutureExt;
use hito_core::test_support::hang_guard::{HANG_CEILING, hang_guard};
use hito_core::{Credentials, Error, InstallPlugin, OfferPluginService, ResolveSource};
use hito_plugin_sdk::{
    Manifest, ProviderRecording, RecordingRequest, SearchPage, SearchRequest, SourcePage,
};
use hito_testkit::fake_plugin::{FakePlugin, RecordedRequest, Reply};
use serde_json::{Value, json};
use std::panic::{AssertUnwindSafe, resume_unwind};
use uuid::Uuid;

const SEARCH: &str = "v1/search";
const DETAIL: &str = "v1/recordings/detail";
const SOURCES: &str = "v1/sources";
const CANDIDATE: &str = "owned-disposition-candidate";
const SOURCE: &str = "owned-disposition-audio";
const PRIVATE_TITLE: &str = "private-disposition-title-never-publish";
const REFUSAL: &str = "Owned explicit disposition refusal";

#[derive(Clone, Copy)]
enum Case {
    Unavailable,
    Refused,
}

#[tokio::test]
async fn provider_failure_dispositions_distinguish_transient_outage_from_explicit_refusal_and_replay_exactly()
 {
    let mut decisions = Vec::new();
    for case in [Case::Unavailable, Case::Refused] {
        let (primary, previous_audio, mut provider) =
            hang_guard("owned disposition services", async {
                (
                    saved::ProviderServer::start().await,
                    saved::ProviderServer::start().await,
                    provider(case).await,
                )
            })
            .await;
        let result = tokio::time::timeout(
            HANG_CEILING,
            AssertUnwindSafe(Box::pin(exercise(
                case,
                &primary,
                &previous_audio,
                &provider,
            )))
            .catch_unwind(),
        )
        .await;
        let joined = provider.finish().await;
        primary.shutdown().await;
        previous_audio.shutdown().await;
        assert!(
            joined,
            "the owned disposition provider must finish before the result assertion"
        );
        decisions.push(match result {
            Ok(Ok(decision)) => decision,
            Ok(Err(panic)) => resume_unwind(panic),
            Err(_) => {
                panic!("the owned disposition workflow must settle within its existing bound")
            }
        });
    }
    for decision in &decisions {
        assert_eq!(
            decision["outcome"], "provider_unavailable",
            "both failed exchanges must retain the existing coarse provider outcome"
        );
    }
    let observed = decisions
        .iter()
        .map(|decision| {
            json!({
                "reason":decision["failure"]["reason"],
                "retryable":decision["failure"]["retryable"],
            })
        })
        .collect::<Vec<_>>();
    assert_eq!(
        observed,
        vec![
            json!({"reason":"provider_unavailable","retryable":true}),
            json!({"reason":"provider_refused","retryable":false}),
        ],
        "public accepted failure decisions must distinguish a transient503 from an explicit valid422 refusal"
    );
}

async fn exercise(
    case: Case,
    primary: &saved::ProviderServer,
    previous_audio: &saved::ProviderServer,
    provider: &FakePlugin,
) -> Value {
    let mut owned = hang_guard(
        "owned disposition recording",
        Box::pin(saved::SavedRecording::prepare(primary, previous_audio)),
    )
    .await;
    owned.prepare_private_title(PRIVATE_TITLE).await;
    owned
        .hito
        .plugins
        .remove(
            &owned.actor,
            &owned.audio_installation.id,
            owned.audio_installation.revision,
        )
        .await
        .unwrap();
    let service = owned
        .hito
        .plugins
        .offer_service(
            &owned.actor,
            OfferPluginService {
                name: "Owned disposition provider".into(),
                base_url: provider.url.clone(),
            },
        )
        .await
        .unwrap();
    let installed = owned
        .hito
        .plugins
        .install(
            &owned.actor,
            InstallPlugin {
                service_id: service.id,
                service_revision: service.revision,
                bearer_token: None,
                accept_manifest_digest: None,
            },
        )
        .await
        .unwrap();
    let connections = owned.hito.plugins.connections(&owned.actor).await.unwrap();
    let eligible = connections
        .installations
        .iter()
        .filter(|installation| {
            installation
                .manifest
                .capabilities
                .contains(&hito_plugin_sdk::Capability::Search)
                && installation
                    .manifest
                    .capabilities
                    .contains(&hito_plugin_sdk::Capability::Sources)
        })
        .collect::<Vec<_>>();
    assert!(
        eligible.len() == 1 && eligible[0].id == installed.id,
        "only the owned disposition provider may participate in discovery"
    );
    owned
        .hito
        .identity
        .create_user(
            &owned.actor,
            Credentials {
                username: "disposition-peer".into(),
                password: "owned disposition peer password".into(),
            },
        )
        .await
        .unwrap();
    let session = owned
        .hito
        .identity
        .login(Credentials {
            username: "disposition-peer".into(),
            password: "owned disposition peer password".into(),
        })
        .await
        .unwrap();
    let peer = owned
        .hito
        .identity
        .authenticate(&session.token)
        .await
        .unwrap();
    let peer_before = json!({
        "connections":peer_connections(&owned.hito, &peer).await,
        "preferences":owned.hito.catalog.source_preferences(&peer).await.unwrap(),
        "recordings":owned.hito.catalog.recordings(&peer, Default::default()).await.unwrap(),
        "activity":owned.hito.collection.activity(&peer).await.unwrap(),
    });
    let choice = serde_json::to_value(
        owned
            .hito
            .catalog
            .source_choice(&owned.actor, &owned.recording)
            .await
            .unwrap(),
    )
    .unwrap();
    assert_eq!(choice["mode"], "automatic");
    let request_id = Uuid::new_v4();
    let resolution = owned
        .hito
        .catalog
        .resolve_source_until(
            &owned.actor,
            &owned.recording,
            ResolveSource { request_id },
            std::future::pending(),
        )
        .await
        .unwrap();
    assert!(
        resolution.request_id == request_id
            && resolution.recording_id == owned.recording
            && resolution.sources.is_empty(),
        "a failed exchange must not publish a source or change identities"
    );
    let initial = serde_json::to_value(&resolution).unwrap();
    let calls = provider.requests();
    assert!(
        calls.len() == 2
            && provider.requests_for(SEARCH).len() == 1
            && valid_search(&provider.requests_for(SEARCH)[0]),
        "one valid real search must follow the manifest with no candidate or audio work"
    );
    let replay = owned
        .hito
        .catalog
        .resolve_source_until(
            &owned.actor,
            &owned.recording,
            ResolveSource { request_id },
            std::future::pending(),
        )
        .await
        .unwrap();
    assert!(
        serde_json::to_value(replay).unwrap() == initial,
        "the same UUID must replay the exact accepted failure decision"
    );
    assert_eq!(
        provider.requests().len(),
        calls.len(),
        "accepted replay must make no new provider exchange"
    );
    assert!(
        matches!(
            owned
                .hito
                .catalog
                .resolve_source_until(
                    &peer,
                    &owned.recording,
                    ResolveSource { request_id },
                    std::future::pending()
                )
                .await,
            Err(Error::NotFound)
        ),
        "a peer must not observe or replay another account's failure receipt"
    );
    assert!(
        serde_json::to_value(
            owned
                .hito
                .catalog
                .source_choice(&owned.actor, &owned.recording)
                .await
                .unwrap()
        )
        .unwrap()
            == choice,
        "a failed exchange and replay must preserve the source choice"
    );
    owned.assert_preserved().await;
    owned.assert_original_preserved();

    if matches!(case, Case::Unavailable) {
        let page = ready_search();
        provider.on(SEARCH, move |request| {
            if !valid_search(request) {
                return Reply::Status(axum::http::StatusCode::BAD_REQUEST);
            }
            Reply::Json(page.clone())
        });
        let fresh_id = Uuid::new_v4();
        let recovered = owned
            .hito
            .catalog
            .resolve_source_until(
                &owned.actor,
                &owned.recording,
                ResolveSource {
                    request_id: fresh_id,
                },
                std::future::pending(),
            )
            .await
            .unwrap();
        let recovered = serde_json::to_value(recovered).unwrap();
        assert!(
            recovered["outcome"] == "ready"
                && recovered["request_id"] == fresh_id.to_string()
                && recovered["resolution_id"] != initial["resolution_id"]
                && recovered["recording_id"] == owned.recording
                && recovered["sources"]
                    .as_array()
                    .is_some_and(|sources| sources.len() == 1 && sources[0]["ready"] == true),
            "a new UUID must recover from503 immediately through a strict valid candidate"
        );
        let after_recovery = provider.requests().len();
        let replay = owned
            .hito
            .catalog
            .resolve_source_until(
                &owned.actor,
                &owned.recording,
                ResolveSource { request_id },
                std::future::pending(),
            )
            .await
            .unwrap();
        let replay = serde_json::to_value(replay).unwrap();
        for field in [
            "request_id",
            "resolution_id",
            "recording_id",
            "outcome",
            "failure",
        ] {
            assert!(
                replay[field] == initial[field],
                "fresh recovery must not rewrite a prior accepted failure decision"
            );
        }
        assert_eq!(
            provider.requests().len(),
            after_recovery,
            "replay after fresh recovery must not dispatch another exchange"
        );
        assert!(
            provider.requests_for(SEARCH).len() == 2
                && provider.requests_for(DETAIL).len() == 1
                && provider.requests_for(SOURCES).len() == 1,
            "only the new UUID may hydrate one strict candidate after the failed search"
        );
    } else {
        assert!(
            provider.requests_for(SEARCH).len() == 1
                && provider.requests_for(DETAIL).is_empty()
                && provider.requests_for(SOURCES).is_empty(),
            "the explicit-refusal receipt must not itself retry or hydrate a candidate"
        );
    }
    assert!(
        provider.health_requests().is_empty()
            && provider.unexpected() == 0
            && provider
                .requests()
                .iter()
                .all(|request| request.handled == hito_testkit::fake_plugin::Handled::Yes)
            && provider.requests_for("v1/audio").is_empty(),
        "the disposition exercise must contain only approved valid protocol work and no audio request"
    );
    for path in [
        SEARCH,
        DETAIL,
        SOURCES,
        "v1/recordings/metadata",
        "v1/audio",
    ] {
        assert!(
            previous_audio.requests(path).await.is_empty(),
            "the removed setup provider must never participate in disposition or recovery"
        );
    }
    assert!(
        json!({
            "connections":peer_connections(&owned.hito, &peer).await,
            "preferences":owned.hito.catalog.source_preferences(&peer).await.unwrap(),
            "recordings":owned.hito.catalog.recordings(&peer, Default::default()).await.unwrap(),
            "activity":owned.hito.collection.activity(&peer).await.unwrap(),
        }) == peer_before,
        "provider decisions and recovery must preserve the peer's private state"
    );
    owned.assert_preserved().await;
    owned.assert_original_preserved();
    let serialized = serde_json::to_string(&initial).unwrap();
    for private in [
        PRIVATE_TITLE,
        REFUSAL,
        CANDIDATE,
        SOURCE,
        &primary.url,
        &previous_audio.url,
        &provider.url,
    ] {
        assert!(
            !serialized.contains(private),
            "accepted failure decisions must omit private fixture content"
        );
    }
    initial
}

async fn provider(case: Case) -> FakePlugin {
    let manifest: Manifest = serde_json::from_value(json!({
        "id":"org.hito.disposition-fixture","name":"Owned disposition provider",
        "protocol_version":1,"capabilities":["search","sources"],
    }))
    .unwrap();
    let detail = detail();
    let sources = json!({"items":[{"id":SOURCE,"label":"Owned recovery audio","ready":true}]});
    assert!(
        serde_json::from_value::<ProviderRecording>(detail.clone())
            .unwrap()
            .validate()
            .is_ok()
    );
    assert!(
        serde_json::from_value::<SourcePage>(sources.clone())
            .unwrap()
            .validate()
            .is_ok()
    );
    FakePlugin::builder(manifest)
        .require_protocol()
        .require_json()
        .strict(16)
        .post(SEARCH, move |request| {
            if !valid_search(request) {
                return Reply::Status(axum::http::StatusCode::BAD_REQUEST);
            }
            match case {
                Case::Unavailable => Reply::Status(axum::http::StatusCode::SERVICE_UNAVAILABLE),
                Case::Refused => Reply::Raw {
                    status: axum::http::StatusCode::UNPROCESSABLE_ENTITY,
                    headers: axum::http::HeaderMap::from_iter([(
                        "content-type".parse().unwrap(),
                        "application/json".parse().unwrap(),
                    )]),
                    body: serde_json::to_vec(&json!({"error":REFUSAL})).unwrap(),
                    chunked: false,
                },
            }
        })
        .post(DETAIL, move |request| {
            if !valid_recording(request) {
                return Reply::Status(axum::http::StatusCode::BAD_REQUEST);
            }
            Reply::Json(detail.clone())
        })
        .post(SOURCES, move |request| {
            if !valid_recording(request) {
                return Reply::Status(axum::http::StatusCode::BAD_REQUEST);
            }
            Reply::Json(sources.clone())
        })
        .start()
        .await
}

fn valid_search(request: &RecordedRequest) -> bool {
    serde_json::from_slice::<SearchRequest>(&request.body).is_ok_and(|request| {
        request.validate().is_ok()
            && request.query == "Owned performer Owned tune"
            && request.cursor.is_none()
            && request.limit == 20
    })
}

fn valid_recording(request: &RecordedRequest) -> bool {
    serde_json::from_slice::<RecordingRequest>(&request.body)
        .is_ok_and(|request| request.validate().is_ok() && request.recording_id == CANDIDATE)
}

fn detail() -> Value {
    json!({"id":CANDIDATE,"title":"Owned tune","artist":"Owned performer","duration_ms":1000,
        "release":null,"revision":"owned-disposition-r1"})
}

fn ready_search() -> Value {
    let page = json!({"items":[detail()],"next_cursor":null});
    assert!(
        serde_json::from_value::<SearchPage>(page.clone())
            .unwrap()
            .validate(20)
            .is_ok()
    );
    page
}

async fn peer_connections(services: &hito_core::Services, peer: &hito_core::Actor) -> Value {
    let connections = services.plugins.connections(peer).await.unwrap();
    json!({"services":connections.services,"installations":connections.installations})
}
