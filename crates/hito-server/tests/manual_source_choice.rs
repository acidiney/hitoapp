#[path = "fixtures/manual_choice_http.rs"]
mod choice_http;
#[path = "fixtures/manual_choice_collection.rs"]
mod collection;
#[path = "fixtures/manual_choice_files.rs"]
mod files;
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
    assert_audio, assert_choice, assert_denied_audio, assert_saved_selection, call, connect,
    ready_download, source_named,
};
use collection::{CollectionSnapshot, assert_collection};
use files::ManagedCopy;
use fixture::Provider;
use futures_util::FutureExt;
use serde_json::json;
use status::assert_status;
use std::{panic::AssertUnwindSafe, path::Path};
use uuid::Uuid;

#[tokio::test]
async fn a_manual_binding_choice_downloads_its_own_audio_and_survives_provider_disconnect_and_restart()
 {
    let directory = tempfile::tempdir().unwrap();
    let mut a = Provider::start("a", 0).await;
    let mut b = Provider::start("b", 777).await;
    assert!(
        a.audio() != b.audio(),
        "the owned PCM representations must have different bytes"
    );
    let mut hosts = Vec::new();
    let outcome = AssertUnwindSafe(exercise(directory.path(), &a, &b, &mut hosts))
        .catch_unwind()
        .await;
    for server in &mut hosts {
        server.request_shutdown();
    }
    let mut joined = true;
    for server in &mut hosts {
        joined &= server.join().await;
    }
    joined &= a.finish().await;
    joined &= b.finish().await;
    assert!(
        joined,
        "all real hosts and owned providers must join on success or panic"
    );
    if let Err(panic) = outcome {
        std::panic::resume_unwind(panic);
    }
}

async fn exercise(directory: &Path, a: &Provider, b: &Provider, hosts: &mut Vec<host::Host>) {
    hosts.push(host::Host::start(host::open(directory, "manual-choice-host").await).await);
    let address = hosts[0].address;
    let session = call(
        address,
        "POST",
        "/api/v1/setup",
        json!({"username": "owner", "password": "owned manual choice password"}),
        None,
        StatusCode::CREATED,
    )
    .await;
    let token = session["token"].as_str().unwrap();
    let installation_a = connect(address, token, a).await;
    let original = call(address, "POST", &format!("/api/v1/plugins/{}/recordings", installation_a["id"].as_str().unwrap()), json!({"installation_revision": installation_a["revision"], "provider_recording_id": a.recording_id()}), Some(token), StatusCode::OK).await;
    let recording = original["id"].as_str().unwrap();
    let sources_path = format!("/api/v1/recordings/{recording}/sources");
    let initial_sources = http::get(address, token, &sources_path).await;
    let source_a = initial_sources[0]["id"].as_str().unwrap().to_owned();
    assert!(initial_sources[0]["ready"] == true);
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
    let saved_a_source = saved_a["source_id"].as_str().unwrap().to_owned();
    assert_audio(address, token, &saved_a_source, a.audio()).await;
    let retained_a = ManagedCopy::capture(directory, a.audio());
    assert_status(address, token, &saved_a, 0, true, [a, b]).await;
    assert!(a.audio_requests() > 0);
    assert_eq!(b.audio_requests(), 0);

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
    assert!(attached["id"] == recording);
    let alternates = http::get(address, token, &sources_path).await;
    assert_eq!(
        alternates
            .as_array()
            .unwrap()
            .iter()
            .filter(|source| source["ready"] == true)
            .count(),
        3,
        "adding an alternate must preserve ordinary eligibility until the explicit choice"
    );
    let source_b = source_named(&alternates, "Owned source b");
    let choice_path = format!("/api/v1/recordings/{recording}/source-choice");
    Uuid::parse_str(&source_b).unwrap();
    let choice_body = json!({"request_id": Uuid::new_v4(), "source_id": source_b});
    let before_choice = (a.requests(), b.requests());
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
    let encoded_choice = choice.to_string();
    for private in [
        a.recording_id(),
        b.recording_id(),
        a.source_id(),
        b.source_id(),
        a.url.clone(),
        b.url.clone(),
    ] {
        assert!(
            !encoded_choice.contains(&private),
            "choice receipts must omit provider identifiers and endpoints"
        );
    }
    assert_eq!(
        (a.requests(), b.requests()),
        before_choice,
        "choosing a known source must not contact providers"
    );
    let replay = call(
        address,
        "POST",
        &choice_path,
        choice_body.clone(),
        Some(token),
        StatusCode::OK,
    )
    .await;
    assert!(
        replay == choice,
        "choice receipts must replay without increasing the revision"
    );
    call(
        address,
        "POST",
        &choice_path,
        json!({"request_id": choice_body["request_id"], "source_id": source_a}),
        Some(token),
        StatusCode::CONFLICT,
    )
    .await;
    assert_eq!(
        (a.requests(), b.requests()),
        before_choice,
        "choice acknowledgement replay and conflicts must not call providers"
    );
    let pinned = http::get(address, token, &sources_path).await;
    assert_eq!(
        pinned
            .as_array()
            .unwrap()
            .iter()
            .filter(|source| source["ready"] == true)
            .count(),
        1
    );
    assert!(
        pinned
            .as_array()
            .unwrap()
            .iter()
            .any(|source| source["id"] == source_b && source["ready"] == true)
    );
    assert_denied_audio(address, token, &source_a).await;
    assert_denied_audio(address, token, &saved_a_source).await;
    assert_status(address, token, &saved_a, 1, false, [a, b]).await;

    b.replace_unavailable();
    let unavailable = call(
        address,
        "POST",
        &format!("{sources_path}/refresh"),
        json!({}),
        Some(token),
        StatusCode::OK,
    )
    .await;
    assert!(
        unavailable
            .as_array()
            .unwrap()
            .iter()
            .all(|source| source["id"] != source_b),
        "the original B representation must have been replaced"
    );
    assert!(
        unavailable
            .as_array()
            .unwrap()
            .iter()
            .all(|source| source["ready"] == false),
        "neither active A nor saved A is a fallback for the unavailable chosen binding"
    );
    let replacement_b = source_named(&unavailable, "Owned source b");
    assert!(
        replacement_b != source_b,
        "the manual choice must follow binding lineage across representation changes"
    );
    assert_collection(address, token, recording, &collection, false).await;
    call(
        address,
        "POST",
        &download_path,
        json!({"request_id": Uuid::new_v4()}),
        Some(token),
        StatusCode::CONFLICT,
    )
    .await;
    assert_eq!(
        b.audio_requests(),
        0,
        "no new persistent download may begin until an eligible source and an explicit Download request exist"
    );
    assert_denied_audio(address, token, &source_a).await;
    assert_denied_audio(address, token, &saved_a_source).await;
    retained_a.assert_retained(a.audio());
    assert_status(address, token, &saved_a, 1, false, [a, b]).await;

    b.make_ready();
    let ready = call(
        address,
        "POST",
        &format!("{sources_path}/refresh"),
        json!({}),
        Some(token),
        StatusCode::OK,
    )
    .await;
    assert!(
        ready
            .as_array()
            .unwrap()
            .iter()
            .any(|source| source["id"] == replacement_b && source["ready"] == true)
    );
    let download_b = call(
        address,
        "POST",
        &download_path,
        json!({"request_id": Uuid::new_v4()}),
        Some(token),
        StatusCode::ACCEPTED,
    )
    .await;
    assert!(
        download_b["intent_id"] != download_a["intent_id"],
        "the explicit B download must not reuse the retained A transfer"
    );
    let saved_b = ready_download(address, token, &download_path, &download_b).await;
    let saved_b_source = saved_b["source_id"].as_str().unwrap();
    assert!(saved_b_source != saved_a_source);
    assert!(
        b.audio_requests() > 0,
        "the selected provider representation must actually be fetched"
    );
    assert_audio(address, token, saved_b_source, b.audio()).await;
    retained_a.assert_replaced(b.audio()).await;
    assert_status(address, token, &saved_b, 1, true, [a, b]).await;
    let audio_control = (a.audio_requests(), b.audio_requests());
    let request_control = (a.requests(), b.requests());

    call(
        address,
        "DELETE",
        &format!("/api/v1/plugins/{}", installation_b["id"].as_str().unwrap()),
        json!({"revision": installation_b["revision"]}),
        Some(token),
        StatusCode::NO_CONTENT,
    )
    .await;
    b.offline();
    assert_saved_selection(address, token, &sources_path, saved_b_source, &source_a).await;
    assert_audio(address, token, saved_b_source, b.audio()).await;
    assert_status(address, token, &saved_b, 1, true, [a, b]).await;
    assert_eq!(
        (a.audio_requests(), b.audio_requests()),
        audio_control,
        "offline playback must read the verified B copy without contacting A or B"
    );
    assert_eq!(
        (a.requests(), b.requests()),
        request_control,
        "disconnect and saved playback must not attempt any provider request"
    );
    hosts[0].request_shutdown();
    assert!(
        hosts[0].join().await,
        "the original real host must finish before restart"
    );
    hosts.push(host::Host::start(host::open(directory, "manual-choice-host").await).await);
    let restarted = hosts.last().unwrap().address;
    let replay = call(
        restarted,
        "POST",
        &choice_path,
        choice_body,
        Some(token),
        StatusCode::OK,
    )
    .await;
    assert!(
        replay == choice,
        "a stored choice acknowledgement must survive restart and provider disconnection"
    );
    assert_saved_selection(restarted, token, &sources_path, saved_b_source, &source_a).await;
    assert_audio(restarted, token, saved_b_source, b.audio()).await;
    assert_denied_audio(restarted, token, &source_a).await;
    assert_status(restarted, token, &saved_b, 1, true, [a, b]).await;
    assert_collection(restarted, token, recording, &collection, true).await;
    assert_eq!((a.audio_requests(), b.audio_requests()), audio_control);
    assert_eq!((a.unexpected(), b.unexpected()), (0, 0));
    let settings = http::get(restarted, token, "/api/v1/plugins").await;
    assert!(
        settings["installations"]
            .as_array()
            .unwrap()
            .iter()
            .any(|installation| installation["id"] == installation_a["id"]),
        "A must remain a live configured control"
    );
    assert_eq!(
        (a.requests(), b.requests()),
        request_control,
        "restart, receipt replay and saved collection reads must not attempt any provider request"
    );
}
