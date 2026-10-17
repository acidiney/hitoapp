#[path = "fixtures/manual_source.rs"]
mod fixture;
#[path = "fixtures/download_host.rs"]
mod host;
#[allow(dead_code)]
#[path = "fixtures/artwork_http.rs"]
mod http;

use axum::http::StatusCode;
use fixture::Provider;
use futures_util::FutureExt;
use serde_json::{Value, json};
use std::{net::SocketAddr, panic::AssertUnwindSafe, path::Path, time::Duration};
use uuid::Uuid;

struct CollectionSnapshot {
    playlist: String,
    detail: Value,
    metadata: Value,
    activity: Value,
}

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
    assert!(a.audio_requests() > 0);
    assert_eq!(b.audio_requests(), 0);

    let playlist = Uuid::new_v4().to_string();
    let entries = [Uuid::new_v4().to_string(), Uuid::new_v4().to_string()];
    preserve_collection(address, token, recording, &playlist, &entries).await;
    let metadata_path = format!("/api/v1/recordings/{recording}/metadata");
    let metadata = http::get(address, token, &metadata_path).await;
    assert!(metadata["selected_observation"]["id"].is_string());
    let before_activity = http::get(address, token, "/api/v1/activity").await;
    let before_playlist = http::get(address, token, &format!("/api/v1/playlists/{playlist}")).await;
    let collection = CollectionSnapshot {
        playlist: playlist.clone(),
        detail: before_playlist,
        metadata,
        activity: before_activity,
    };

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

async fn connect(address: SocketAddr, token: &str, provider: &Provider) -> Value {
    let service = call(
        address,
        "POST",
        "/api/v1/plugins/services",
        json!({"name": "Owned manual source provider", "base_url": provider.url}),
        Some(token),
        StatusCode::CREATED,
    )
    .await;
    call(
        address,
        "POST",
        "/api/v1/plugins",
        json!({"service_id": service["id"], "service_revision": service["revision"]}),
        Some(token),
        StatusCode::CREATED,
    )
    .await
}

async fn preserve_collection(
    address: SocketAddr,
    token: &str,
    recording: &str,
    playlist: &str,
    entries: &[String; 2],
) {
    let actions = [
        json!({"kind":"playlist_create", "playlist_id":playlist, "name":"Owned duplicate playlist"}),
        json!({"kind":"entry_insert", "playlist_id":playlist, "entry_id":entries[0], "recording_id":recording, "position":0}),
        json!({"kind":"entry_insert", "playlist_id":playlist, "entry_id":entries[1], "recording_id":recording, "position":1}),
        json!({"kind":"recording_title_set", "recording_id":recording, "title":"Private corrected title"}),
        json!({"kind":"recording_artist_set", "recording_id":recording, "artist":"Private corrected performer"}),
        json!({"kind":"like_set", "recording_id":recording, "liked":true}),
        json!({"kind":"progress_set", "recording_id":recording, "position_ms":431, "speed":1.25}),
    ];
    let operations: Vec<Value> = actions
        .into_iter()
        .map(|action| json!({"id":Uuid::new_v4(), "action":action}))
        .collect();
    let receipt = call(
        address,
        "POST",
        "/api/v1/sync",
        json!({"device_id":"manual-choice-client", "operations":operations}),
        Some(token),
        StatusCode::OK,
    )
    .await;
    assert_eq!(receipt["acknowledged"].as_array().unwrap().len(), 7);
}

async fn ready_download(address: SocketAddr, token: &str, path: &str, receipt: &Value) -> Value {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
    loop {
        let response = http::call(address, "GET", path, None, Some(token)).await;
        assert_eq!(response.status, StatusCode::OK);
        let status = response.json();
        assert!(
            status["intent_id"] == receipt["intent_id"],
            "status must belong to the current explicit download request"
        );
        if status["status"] == "ready" {
            assert!(status["source_id"].is_string());
            return status;
        }
        assert!(
            matches!(
                status["status"].as_str(),
                Some("queued" | "running" | "verifying")
            ),
            "owned download must not fail"
        );
        assert!(
            tokio::time::Instant::now() < deadline,
            "owned deterministic download must settle within the test deadline"
        );
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

fn source_named(sources: &Value, label: &str) -> String {
    sources
        .as_array()
        .unwrap()
        .iter()
        .find(|source| source["label"].as_str() == Some(label))
        .expect("the owned provider source must be listed")["id"]
        .as_str()
        .unwrap()
        .to_owned()
}

fn assert_choice(choice: &Value, recording: &str, request: &Value) {
    let mut keys: Vec<&str> = choice
        .as_object()
        .unwrap()
        .keys()
        .map(String::as_str)
        .collect();
    keys.sort_unstable();
    assert_eq!(
        keys,
        ["recording_id", "request_id", "revision", "source_id"]
    );
    assert!(
        choice["request_id"] == request["request_id"]
            && choice["source_id"] == request["source_id"]
            && choice["recording_id"] == recording
    );
    assert!(
        choice["revision"]
            .as_u64()
            .is_some_and(|revision| revision > 0)
    );
}

async fn assert_saved_selection(
    address: SocketAddr,
    token: &str,
    sources_path: &str,
    saved: &str,
    a: &str,
) {
    let sources = http::get(address, token, sources_path).await;
    let ready: Vec<&Value> = sources
        .as_array()
        .unwrap()
        .iter()
        .filter(|source| source["ready"] == true)
        .collect();
    assert_eq!(ready.len(), 1);
    assert!(
        ready[0]["id"] == saved && ready[0]["provider"] == "download",
        "only the selected binding's verified saved copy is eligible"
    );
    assert!(
        sources
            .as_array()
            .unwrap()
            .iter()
            .any(|source| source["id"] == a && source["ready"] == false),
        "active A must remain retained and ineligible"
    );
}

async fn assert_collection(
    address: SocketAddr,
    token: &str,
    recording: &str,
    snapshot: &CollectionSnapshot,
    available: bool,
) {
    let playlist = &snapshot.playlist;
    let original_playlist = &snapshot.detail;
    let original_metadata = &snapshot.metadata;
    let original_activity = &snapshot.activity;
    let metadata = http::get(
        address,
        token,
        &format!("/api/v1/recordings/{recording}/metadata"),
    )
    .await;
    assert!(
        metadata["selected_observation"] == original_metadata["selected_observation"]
            && metadata["title"] == original_metadata["title"]
            && metadata["artist"] == original_metadata["artist"],
        "source choice must preserve primary and personal metadata"
    );
    let current = http::get(address, token, &format!("/api/v1/playlists/{playlist}")).await;
    assert!(
        current["playlist"] == original_playlist["playlist"]
            && current["origin"] == original_playlist["origin"]
    );
    let entries = current["entries"].as_array().unwrap();
    let before = original_playlist["entries"].as_array().unwrap();
    assert_eq!(entries.len(), 2);
    for (entry, original) in entries.iter().zip(before) {
        for field in [
            "entry_id",
            "recording_id",
            "position",
            "title",
            "artist",
            "inherited_title",
            "inherited_artist",
            "duration_ms",
            "origin",
        ] {
            assert!(
                entry[field] == original[field],
                "duplicate playlist identity and personal fields must survive source changes"
            );
        }
        assert!(entry["liked"] == true && entry["available"] == available);
    }
    let activity = http::get(address, token, "/api/v1/activity").await;
    let progress = activity["history"]
        .as_array()
        .unwrap()
        .iter()
        .find(|item| item["id"] == recording)
        .unwrap();
    let original = original_activity["history"]
        .as_array()
        .unwrap()
        .iter()
        .find(|item| item["id"] == recording)
        .unwrap();
    for field in [
        "id",
        "position_ms",
        "speed",
        "updated_at",
        "title",
        "artist",
        "liked",
    ] {
        assert!(
            progress[field] == original[field],
            "progress and private collection state must survive source changes"
        );
    }
    assert!(progress["available"] == available);
    let recordings = http::get(address, token, "/api/v1/recordings").await;
    let saved = recordings["items"].as_array().unwrap();
    assert_eq!(
        saved.len(),
        1,
        "choosing audio must not adopt a second recording"
    );
    assert!(
        saved[0]["id"] == recording
            && saved[0]["liked"] == true
            && saved[0]["available"] == available
    );
}

async fn assert_audio(address: SocketAddr, token: &str, source: &str, expected: &[u8]) {
    let response = http::call(
        address,
        "GET",
        &format!("/api/v1/sources/{source}/audio"),
        None,
        Some(token),
    )
    .await;
    assert_eq!(response.status, StatusCode::OK);
    assert!(
        response.body == expected,
        "actual relayed bytes must belong to the explicitly chosen representation"
    );
}

async fn assert_denied_audio(address: SocketAddr, token: &str, source: &str) {
    let response = http::call(
        address,
        "GET",
        &format!("/api/v1/sources/{source}/audio"),
        None,
        Some(token),
    )
    .await;
    assert_eq!(
        response.status,
        StatusCode::NOT_FOUND,
        "an ineligible representation must not bypass the choice through a known Hito source URL"
    );
}

async fn call(
    address: SocketAddr,
    method: &str,
    path: &str,
    body: Value,
    token: Option<&str>,
    expected: StatusCode,
) -> Value {
    let response = http::call(address, method, path, Some(body), token).await;
    assert_eq!(response.status, expected);
    if expected == StatusCode::NO_CONTENT {
        Value::Null
    } else {
        response.json()
    }
}
