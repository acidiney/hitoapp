#[allow(dead_code)]
#[path = "fixtures/manual_choice_http.rs"]
mod choice_http;
#[path = "fixtures/manual_choice_collection.rs"]
mod collection;
#[allow(dead_code)]
#[path = "fixtures/manual_choice_files.rs"]
mod files;
#[allow(dead_code)]
#[path = "fixtures/manual_source.rs"]
mod fixture;
#[path = "fixtures/download_host.rs"]
mod host;
#[allow(dead_code)]
#[path = "fixtures/artwork_http.rs"]
mod http;
#[path = "fixtures/manual_choice_status.rs"]
mod status;

use axum::http::StatusCode;
use choice_http::{
    assert_audio, assert_choice, assert_denied_audio, call, connect, ready_download, source_named,
};
use collection::{CollectionSnapshot, assert_collection};
use files::ManagedCopy;
use fixture::Provider;
use futures_util::FutureExt;
use hito_core::{AudioRequest, Error, OpenedAudio, Services};
use serde_json::json;
use status::assert_status;
use std::{net::SocketAddr, panic::AssertUnwindSafe, path::Path, time::Duration};
use uuid::Uuid;

#[tokio::test]
async fn choosing_another_binding_revokes_unread_saved_audio_without_removing_its_bytes() {
    let directory = tempfile::tempdir().unwrap();
    let mut providers = Vec::new();
    let mut hosts = Vec::new();
    let outcome = tokio::time::timeout(
        Duration::from_secs(20),
        AssertUnwindSafe(async {
            providers.push(Provider::start("a", 0).await);
            providers.push(Provider::start("b", 777).await);
            let services = host::open(directory.path(), "held-saved-audio-host").await;
            hosts.push(host::Host::start(services.clone()).await);
            exercise(
                directory.path(),
                hosts[0].address,
                &services,
                &providers[0],
                &providers[1],
            )
            .await;
        })
        .catch_unwind(),
    )
    .await;
    for server in &mut hosts {
        server.request_shutdown();
    }
    let mut joined = true;
    for server in &mut hosts {
        joined &= server.join().await;
    }
    for provider in &mut providers {
        joined &= provider.finish().await;
    }
    assert!(
        joined,
        "all real hosts and owned providers must join on success, panic or timeout"
    );
    match outcome {
        Ok(Ok(())) => {}
        Ok(Err(panic)) => std::panic::resume_unwind(panic),
        Err(_) => {
            panic!("the owned saved-body authority lifecycle must finish within its deadline")
        }
    }
}

async fn exercise(
    directory: &Path,
    address: SocketAddr,
    services: &Services,
    a: &Provider,
    b: &Provider,
) {
    assert!(
        a.audio() != b.audio(),
        "the owned PCM representations must differ"
    );
    let session = call(
        address,
        "POST",
        "/api/v1/setup",
        json!({"username": "owner", "password": "owned held audio password"}),
        None,
        StatusCode::CREATED,
    )
    .await;
    let token = session["token"].as_str().unwrap();
    let actor = services.identity.authenticate(token).await.unwrap();
    let installation_a = connect(address, token, a).await;
    let original = call(
        address,
        "POST",
        &format!("/api/v1/plugins/{}/recordings", installation_a["id"].as_str().unwrap()),
        json!({"installation_revision": installation_a["revision"], "provider_recording_id": a.recording_id()}),
        Some(token),
        StatusCode::OK,
    )
    .await;
    let recording = original["id"].as_str().unwrap();
    let sources_path = format!("/api/v1/recordings/{recording}/sources");
    let initial_sources = http::get(address, token, &sources_path).await;
    let source_a = source_named(&initial_sources, "Owned source a");
    assert!(
        initial_sources.as_array().unwrap().len() == 1
            && initial_sources[0]["provider"] == "plugin"
            && initial_sources[0]["ready"] == true
    );
    let download_path = format!("/api/v1/recordings/{recording}/download");
    let download_a = call(
        address,
        "POST",
        &download_path,
        json!({"request_id": Uuid::new_v4()}),
        Some(token),
        StatusCode::ACCEPTED,
    )
    .await;
    let saved_a = ready_download(address, token, &download_path, &download_a).await;
    let saved_source = saved_a["source_id"].as_str().unwrap();
    assert_audio(address, token, saved_source, a.audio()).await;
    let retained = ManagedCopy::capture(directory, a.audio());
    let collection = CollectionSnapshot::prepare(address, token, recording).await;

    let installation_b = connect(address, token, b).await;
    let candidate = json!({"installation_id": installation_b["id"], "installation_revision": installation_b["revision"], "provider_recording_id": b.recording_id()});
    let review = call(
        address,
        "POST",
        &format!("{sources_path}/review"),
        candidate.clone(),
        Some(token),
        StatusCode::OK,
    )
    .await;
    let attached = call(
        address,
        "POST",
        &format!("{sources_path}/attach"),
        json!({"candidate": candidate, "review_stamp": review["review_stamp"]}),
        Some(token),
        StatusCode::OK,
    )
    .await;
    assert!(
        attached["id"] == recording,
        "the reviewed B source must attach to the existing recording"
    );
    let alternates = http::get(address, token, &sources_path).await;
    let source_b = source_named(&alternates, "Owned source b");
    for (id, provider) in [
        (source_a.as_str(), "plugin"),
        (saved_source, "download"),
        (source_b.as_str(), "plugin"),
    ] {
        assert!(
            alternates
                .as_array()
                .unwrap()
                .iter()
                .any(|source| source["id"] == id
                    && source["provider"] == provider
                    && source["ready"] == true),
            "A, its verified saved copy and B must be eligible before the explicit choice"
        );
    }
    assert_status(address, token, &saved_a, 0, true, [a, b]).await;
    assert!(
        a.audio_requests() > 0 && b.audio_requests() == 0,
        "only the explicit A download may fetch provider audio"
    );
    let counters = (
        a.requests(),
        b.requests(),
        a.audio_requests(),
        b.audio_requests(),
    );
    let OpenedAudio::Content(mut opened) = services
        .catalog
        .open_audio(&actor, saved_source, AudioRequest::default())
        .await
        .unwrap()
    else {
        panic!("the authorized saved A body must open before the choice");
    };
    assert!(
        opened.content_type == "audio/wav"
            && opened.length == Some(a.audio().len() as u64)
            && opened.seekable,
        "the held body must describe the verified local A copy"
    );
    let mut held = opened
        .body
        .take()
        .expect("the saved A request must expose an unread body");

    let choice_path = format!("/api/v1/recordings/{recording}/source-choice");
    let choice_body = json!({"request_id": Uuid::new_v4(), "source_id": source_b});
    let choice = call(
        address,
        "POST",
        &choice_path,
        choice_body.clone(),
        Some(token),
        StatusCode::OK,
    )
    .await;
    assert_choice(&choice, recording, &choice_body);
    assert!(choice["revision"] == 1);
    let encoded = choice.to_string();
    for private in [
        a.recording_id(),
        b.recording_id(),
        a.source_id(),
        b.source_id(),
        a.url.clone(),
        b.url.clone(),
    ] {
        assert!(
            !encoded.contains(&private),
            "the public choice receipt must omit provider identities and endpoints"
        );
    }
    let current = http::get(address, token, &choice_path).await;
    assert!(
        current["mode"] == "manual"
            && current["revision"] == 1
            && current["sources"]
                .as_array()
                .unwrap()
                .iter()
                .any(|entry| entry["source"]["id"] == source_b && entry["selected"] == true),
        "the accepted HTTP choice must select B in the shared core services"
    );
    assert!(
        matches!(
            services
                .catalog
                .open_audio(&actor, saved_source, AudioRequest::default())
                .await,
            Err(Error::NotFound)
        ),
        "a fresh core open of saved A must fail after the B choice"
    );
    assert_denied_audio(address, token, &source_a).await;
    assert_denied_audio(address, token, saved_source).await;
    retained.assert_retained(a.audio());
    assert_status(address, token, &saved_a, 1, false, [a, b]).await;
    assert_collection(address, token, recording, &collection, true).await;
    assert_eq!(
        (
            a.requests(),
            b.requests(),
            a.audio_requests(),
            b.audio_requests()
        ),
        counters,
        "opening saved audio, choosing B and reading local state must not contact either provider"
    );

    assert!(
        held.chunk().await.is_err(),
        "the first unread saved A chunk must fail after B is accepted, before yielding any A bytes"
    );
    assert_eq!(
        (
            a.requests(),
            b.requests(),
            a.audio_requests(),
            b.audio_requests()
        ),
        counters,
        "rejecting a held local body must not contact providers"
    );
    let reset_body = json!({"request_id": Uuid::new_v4(), "expected_revision": 1});
    let reset = call(
        address,
        "POST",
        &format!("{choice_path}/reset"),
        reset_body.clone(),
        Some(token),
        StatusCode::OK,
    )
    .await;
    assert!(
        reset
            == json!({"request_id": reset_body["request_id"], "recording_id": recording, "revision": 2}),
        "the reset must return its exact immutable public receipt"
    );
    let automatic = http::get(address, token, &choice_path).await;
    assert!(automatic["mode"] == "automatic" && automatic["revision"] == 2);
    assert!(
        matches!(held.chunk().await, Ok(None)),
        "a revoked body must remain terminal after a later reset authorizes a new A stream"
    );
    assert_audio(address, token, saved_source, a.audio()).await;
    retained.assert_retained(a.audio());
    assert_status(address, token, &saved_a, 2, true, [a, b]).await;
    assert_collection(address, token, recording, &collection, true).await;
    assert_eq!(
        (
            a.requests(),
            b.requests(),
            a.audio_requests(),
            b.audio_requests()
        ),
        counters,
        "reset, terminal-body checks and a new saved A stream must stay entirely local"
    );
    assert_eq!((a.unexpected(), b.unexpected()), (0, 0));
}
