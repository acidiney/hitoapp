#[allow(dead_code)]
#[path = "../fixtures/manual_choice_http.rs"]
mod choice_http;
#[path = "../fixtures/manual_choice_collection.rs"]
mod collection;
#[path = "../fixtures/source_choice_controls.rs"]
mod fixture;
#[path = "../fixtures/download_host.rs"]
mod host;
#[allow(dead_code)]
#[path = "../fixtures/artwork_http.rs"]
mod http;

use axum::http::StatusCode;
use choice_http::{assert_audio, assert_choice, call, connect, source_named};
use collection::{CollectionSnapshot, assert_collection};
use fixture::{Provider, Role};
use futures_util::FutureExt;
use serde_json::{Value, json};
use std::{net::SocketAddr, panic::AssertUnwindSafe};
use uuid::Uuid;

#[tokio::test]
async fn returning_to_automatic_preserves_reviewed_history_and_old_receipts_do_not_reapply_choices()
{
    let directory = tempfile::tempdir().unwrap();
    let mut a = Provider::start(Role::Primary).await;
    let mut b = Provider::start(Role::Reviewed).await;
    let mut c = Provider::start(Role::Automatic).await;
    let mut hosts = Vec::new();
    let outcome = AssertUnwindSafe(async {
        hosts.push(
            host::Host::start(host::open(directory.path(), "source-controls-host").await).await,
        );
        exercise(hosts[0].address, &a, &b, &c).await;
    })
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
    joined &= c.finish().await;
    assert!(
        joined,
        "all real hosts and owned providers must join on success or panic"
    );
    if let Err(panic) = outcome {
        std::panic::resume_unwind(panic);
    }
}

async fn exercise(address: SocketAddr, a: &Provider, b: &Provider, c: &Provider) {
    let session = call(
        address,
        "POST",
        "/api/v1/setup",
        json!({"username": "owner", "password": "owned source controls password"}),
        None,
        StatusCode::CREATED,
    )
    .await;
    let token = session["token"].as_str().unwrap();
    let installation_a = connect(address, token, a).await;
    let original = call(address, "POST", &format!("/api/v1/plugins/{}/recordings", installation_a["id"].as_str().unwrap()), json!({"installation_revision": installation_a["revision"], "provider_recording_id": a.recording_id()}), Some(token), StatusCode::OK).await;
    let recording = original["id"].as_str().unwrap();
    assert!(
        original["available"] == false,
        "the primary metadata must have no playable audio"
    );
    let installation_b = connect(address, token, b).await;
    let sources_path = format!("/api/v1/recordings/{recording}/sources");
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
        attached["id"] == recording && attached["available"] == false,
        "the reviewed alternate must preserve the unavailable primary recording"
    );
    let sources = http::get(address, token, &sources_path).await;
    assert!(
        sources.as_array().unwrap().len() == 1
            && sources[0]["provider"] == "plugin"
            && sources[0]["ready"] == false,
        "the reviewed B source must exist as unavailable provider audio"
    );
    let source_b = source_named(&sources, fixture::REVIEWED_LABEL);
    let installation_c = connect(address, token, c).await;
    let collection = CollectionSnapshot::prepare(address, token, recording).await;
    let metadata_path = format!("/api/v1/recordings/{recording}/metadata");
    let metadata = http::get(address, token, &metadata_path).await;
    let observed_b = observations(&metadata, &installation_b);
    assert!(
        observed_b.len() == 1
            && observed_b[0]["provider"] == "org.hito.controls-reviewed"
            && observed_b[0]["provider_recording_id"] == b.recording_id()
            && observed_b[0]["origin"] == "provider_response",
        "B must initially retain exactly its installation-specific reviewed observation"
    );
    assert!(
        metadata["selected_observation"]["plugin_installation_id"] == installation_a["id"],
        "A must remain the selected primary metadata provider"
    );
    let no_io = [a.requests(), b.requests(), c.requests()];
    let choice_path = format!("/api/v1/recordings/{recording}/source-choice");
    let reset_path = format!("{choice_path}/reset");
    let resolve_path = format!("{sources_path}/resolve");

    let controls = ChoiceControls {
        address,
        token,
        recording,
        source_b: &source_b,
    };
    controls.assert_mode("reviewed", 0, 1).await;
    let protected = call(
        address,
        "POST",
        &resolve_path,
        json!({"request_id": Uuid::new_v4()}),
        Some(token),
        StatusCode::OK,
    )
    .await;
    assert!(
        protected["recording_id"] == recording && protected["outcome"] == "protected_choice",
        "legacy reviewed evidence must remain protected before an explicit reset"
    );
    assert!(
        c.searches() == 0 && c.audio_requests() == 0,
        "default reviewed protection must not search or fetch C"
    );
    assert_eq!(
        [a.requests(), b.requests(), c.requests()],
        no_io,
        "choice reads and protected resolution must not attempt any provider request"
    );
    assert_collection(address, token, recording, &collection, false).await;

    let first_choose = json!({"request_id": Uuid::new_v4(), "source_id": source_b});
    let first_choice = call(
        address,
        "POST",
        &choice_path,
        first_choose.clone(),
        Some(token),
        StatusCode::OK,
    )
    .await;
    assert_choice(&first_choice, recording, &first_choose);
    assert!(
        first_choice["revision"] == 1,
        "an unavailable but approved B binding must be selectable"
    );
    controls.assert_mode("manual", 1, 1).await;
    call(
        address,
        "POST",
        &reset_path,
        json!({"request_id": Uuid::new_v4(), "expected_revision": 0}),
        Some(token),
        StatusCode::CONFLICT,
    )
    .await;
    controls.assert_mode("manual", 1, 1).await;

    let first_reset = json!({"request_id": Uuid::new_v4(), "expected_revision": 1});
    let reset_receipt = call(
        address,
        "POST",
        &reset_path,
        first_reset.clone(),
        Some(token),
        StatusCode::OK,
    )
    .await;
    assert_reset(&reset_receipt, recording, &first_reset, 2);
    controls.assert_mode("automatic", 2, 1).await;
    let old_choose = call(
        address,
        "POST",
        &choice_path,
        first_choose.clone(),
        Some(token),
        StatusCode::OK,
    )
    .await;
    assert!(
        old_choose == first_choice,
        "a choice replay must return the immutable old acknowledgement"
    );
    controls.assert_mode("automatic", 2, 1).await;

    let second_choose = json!({"request_id": Uuid::new_v4(), "source_id": source_b});
    let second_choice = call(
        address,
        "POST",
        &choice_path,
        second_choose.clone(),
        Some(token),
        StatusCode::OK,
    )
    .await;
    assert_choice(&second_choice, recording, &second_choose);
    assert!(
        second_choice["revision"] == 3,
        "a new manual selection must advance beyond the reset revision"
    );
    controls.assert_mode("manual", 3, 1).await;
    let old_reset = call(
        address,
        "POST",
        &reset_path,
        first_reset.clone(),
        Some(token),
        StatusCode::OK,
    )
    .await;
    assert!(
        old_reset == reset_receipt,
        "a reset replay must return its old acknowledgement despite the newer current revision"
    );
    controls.assert_mode("manual", 3, 1).await;
    call(
        address,
        "POST",
        &reset_path,
        json!({"request_id": first_choose["request_id"], "expected_revision": 3}),
        Some(token),
        StatusCode::CONFLICT,
    )
    .await;
    call(
        address,
        "POST",
        &choice_path,
        json!({"request_id": first_reset["request_id"], "source_id": source_b}),
        Some(token),
        StatusCode::CONFLICT,
    )
    .await;
    call(
        address,
        "POST",
        &reset_path,
        json!({"request_id": first_reset["request_id"], "expected_revision": 3}),
        Some(token),
        StatusCode::CONFLICT,
    )
    .await;
    controls.assert_mode("manual", 3, 1).await;
    let final_reset = json!({"request_id": Uuid::new_v4(), "expected_revision": 3});
    let automatic_receipt = call(
        address,
        "POST",
        &reset_path,
        final_reset.clone(),
        Some(token),
        StatusCode::OK,
    )
    .await;
    assert_reset(&automatic_receipt, recording, &final_reset, 4);
    controls.assert_mode("automatic", 4, 1).await;
    assert_eq!(
        [a.requests(), b.requests(), c.requests()],
        no_io,
        "read, choose, reset, replay and conflict handling must not attempt provider IO"
    );

    let request_id = Uuid::new_v4();
    let resolution = call(
        address,
        "POST",
        &resolve_path,
        json!({"request_id": request_id}),
        Some(token),
        StatusCode::OK,
    )
    .await;
    assert!(
        resolution["request_id"] == request_id.to_string()
            && resolution["recording_id"] == recording
            && resolution["outcome"] == "ready",
        "an explicit reset must really permit automatic discovery despite retained reviewed B evidence"
    );
    let source_c = source_named(&resolution["sources"], fixture::AUTOMATIC_LABEL);
    assert!(
        c.searches() == 1
            && [a.audio_requests(), b.audio_requests(), c.audio_requests()] == [0, 0, 0],
        "automatic discovery must search C once without fetching audio"
    );
    assert_eq!(
        [a.requests(), b.requests()],
        [no_io[0], no_io[1]],
        "resolution must not send C identifiers or queries to the metadata and reviewed providers"
    );
    let published = [a.requests(), b.requests(), c.requests()];
    let automatic = controls.assert_mode("automatic", 4, 2).await;
    assert!(
        automatic["sources"]
            .as_array()
            .unwrap()
            .iter()
            .any(|entry| entry["source"]["id"] == source_c
                && entry["source"]["provider"] == "plugin"
                && entry["source"]["ready"] == true
                && entry["selectable"] == true
                && entry["selected"] == false),
        "the automatically discovered current C source must be playable and selectable"
    );
    let after = http::get(address, token, &metadata_path).await;
    assert!(
        observations(&after, &installation_b) == observed_b,
        "reset and discovery must preserve the complete reviewed B observation history without retiring and rebinding B"
    );
    let observed_c = observations(&after, &installation_c);
    assert!(
        observed_c.len() == 1
            && observed_c[0]["provider"] == "org.hito.controls-automatic"
            && observed_c[0]["provider_recording_id"] == c.recording_id(),
        "C audio must retain independent provider provenance"
    );
    assert_collection(address, token, recording, &collection, true).await;
    assert_eq!(
        [a.requests(), b.requests(), c.requests()],
        published,
        "published choice and private collection reads must not call providers"
    );
    assert_audio(address, token, &source_c, c.audio()).await;
    assert!(
        c.audio_requests() == 1 && a.audio_requests() == 0 && b.audio_requests() == 0,
        "only the final explicit audio request may fetch the exact owned C representation"
    );
    assert_eq!([a.unexpected(), b.unexpected(), c.unexpected()], [0, 0, 0]);
}

struct ChoiceControls<'a> {
    address: SocketAddr,
    token: &'a str,
    recording: &'a str,
    source_b: &'a str,
}

impl ChoiceControls<'_> {
    async fn assert_mode(&self, mode: &str, revision: u64, count: usize) -> Value {
        let view = http::get(
            self.address,
            self.token,
            &format!("/api/v1/recordings/{}/source-choice", self.recording),
        )
        .await;
        let mut keys: Vec<&str> = view
            .as_object()
            .unwrap()
            .keys()
            .map(String::as_str)
            .collect();
        keys.sort_unstable();
        assert!(
            keys == ["mode", "recording_id", "revision", "sources"],
            "the choice snapshot must contain only the public controls contract"
        );
        assert!(
            view["recording_id"] == self.recording
                && view["revision"] == revision
                && view["mode"] == mode,
            "the current choice mode and revision must match the latest accepted command"
        );
        let sources = view["sources"].as_array().unwrap();
        assert!(
            sources.len() == count,
            "the controls snapshot must preserve current B while allowing the new C source"
        );
        let b = sources
            .iter()
            .find(|entry| entry["source"]["id"] == self.source_b)
            .unwrap_or_else(|| {
                panic!("the same current B Hito source must remain in the controls snapshot")
            });
        let selected = mode == "manual";
        assert!(
            b["source"]["recording_id"] == self.recording
                && b["source"]["provider"] == "plugin"
                && b["source"]["ready"] == false
                && b["selected"] == selected
                && b["selectable"] == true,
            "B must remain selectable and unavailable with its original current source identity"
        );
        if !selected {
            assert!(
                sources.iter().all(|entry| entry["selected"] == false),
                "Automatic and legacy Reviewed modes must have no explicit binding pin"
            );
        }
        for entry in sources {
            let mut keys: Vec<&str> = entry
                .as_object()
                .unwrap()
                .keys()
                .map(String::as_str)
                .collect();
            keys.sort_unstable();
            assert!(
                keys == ["selectable", "selected", "source"],
                "source choices must contain structured sources and selection facts only"
            );
        }
        view
    }
}

fn assert_reset(receipt: &Value, recording: &str, request: &Value, revision: u64) {
    let mut keys: Vec<&str> = receipt
        .as_object()
        .unwrap()
        .keys()
        .map(String::as_str)
        .collect();
    keys.sort_unstable();
    assert!(
        keys == ["recording_id", "request_id", "revision"],
        "reset receipts must contain only immutable Hito acknowledgement fields"
    );
    assert!(
        receipt["recording_id"] == recording
            && receipt["request_id"] == request["request_id"]
            && receipt["revision"] == revision,
        "reset acknowledgement identities and revision must match the accepted command"
    );
}

fn observations(metadata: &Value, installation: &Value) -> Vec<Value> {
    metadata["observations"]["items"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|observation| observation["plugin_installation_id"] == installation["id"])
        .cloned()
        .collect()
}
