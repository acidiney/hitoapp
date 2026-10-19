#![cfg(target_os = "linux")]

#[path = "support/runtime.rs"]
mod runtime;

use runtime::{Host, PendingRequest};
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs::{OpenOptions, Permissions},
    io::Write,
    os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt},
};
use uuid::Uuid;

const ADMIN: &str = "17a0427d104d87ad8adce74f1fbbe334cb4969d697d0fe4d00bc58e15863a5ad";

#[test]
fn committed_revocation_fences_held_corrections_preserves_peer_state_and_historical_admin_receipts()
{
    let fixture = tempfile::tempdir().unwrap();
    std::fs::set_permissions(fixture.path(), Permissions::from_mode(0o700)).unwrap();
    let key = fixture
        .path()
        .join("private-correction-administrator-token");
    let state = fixture.path().join("private-correction-revocation-state");
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&key)
        .unwrap();
    writeln!(file, "{ADMIN}").unwrap();
    drop(file);
    std::fs::set_permissions(&key, Permissions::from_mode(0o400)).unwrap();
    let original_bytes = std::fs::read(&key).unwrap();
    let original_metadata = key.metadata().unwrap();
    let mut host = Host::start(fixture.path(), &state, &key, "correction-revoke");
    host.ready();
    let reporter = provision(&host);
    let peer = provision(&host);
    let token = reporter["token"].as_str().unwrap();
    let peer_token = peer["token"].as_str().unwrap();
    assert_eq!(
        request(
            &host,
            "POST",
            "/v1/corrections",
            Some(token),
            r#"{"private-correction-body-key":"private-correction-body-value"}"#
        ),
        (422, json!({"error": "invalid_input"}))
    );
    let mut private = vec![
        ADMIN.to_owned(),
        token.to_owned(),
        peer_token.to_owned(),
        reporter["reporter_id"].as_str().unwrap().to_owned(),
        peer["reporter_id"].as_str().unwrap().to_owned(),
        fixture.path().to_str().unwrap().to_owned(),
        "private-".to_owned(),
        "PrivTone001".to_owned(),
        "18446744073709551615".to_owned(),
    ];
    let mut owned = Vec::new();
    for _ in 0..3 {
        let claim = suggestion();
        let (status, receipt) = post(&host, "/v1/corrections", token, &claim);
        assert_eq!(status, 201);
        private.push(claim["submission_id"].as_str().unwrap().to_owned());
        private.push(receipt["correction_id"].as_str().unwrap().to_owned());
        owned.push((claim, receipt));
    }
    let mut historical = Vec::new();
    for (index, decision) in [(1, "approve"), (2, "reject")] {
        let path = review_path(&owned[index].1);
        let request = review(1, decision);
        let (status, receipt) = post(&host, &path, ADMIN, &request);
        assert_eq!(status, 201);
        private.push(request["request_id"].as_str().unwrap().to_owned());
        historical.push((path, request, receipt));
    }
    let (status, peer_pending) = post(&host, "/v1/corrections", peer_token, &owned[0].0);
    assert_eq!(status, 201);
    let (status, peer_approved) = post(&host, "/v1/corrections", peer_token, &owned[1].0);
    assert_eq!(status, 201);
    private.extend([
        peer_pending["correction_id"].as_str().unwrap().to_owned(),
        peer_approved["correction_id"].as_str().unwrap().to_owned(),
    ]);
    let peer_path = review_path(&peer_approved);
    assert_eq!(
        request(
            &host,
            "POST",
            &peer_path,
            Some(ADMIN),
            r#"{"private-correction-body-key":"private-correction-body-value"}"#
        ),
        (422, json!({"error": "invalid_input"}))
    );
    let peer_review = review(1, "approve");
    let (status, peer_ack) = post(&host, &peer_path, ADMIN, &peer_review);
    assert_eq!(status, 201);
    private.push(peer_review["request_id"].as_str().unwrap().to_owned());
    let outcome = json!({"submission_id": Uuid::new_v4(), "source": {"provider": "jamendo", "recording_id": "18446744073709551615"}, "outcome": "successful_playback"});
    private.push(outcome["submission_id"].as_str().unwrap().to_owned());
    assert_eq!(post(&host, "/v1/outcomes", token, &outcome).0, 201);
    let (status, peer_outcome) = post(&host, "/v1/outcomes", peer_token, &outcome);
    assert_eq!(status, 201);
    private.push(peer_outcome["receipt_id"].as_str().unwrap().to_owned());
    assert_eq!(rank(&host), 2);

    let baseline = current(&host);
    let known = [
        (&owned[0].1, "pending", 1),
        (&owned[1].1, "approved", 2),
        (&owned[2].1, "rejected", 2),
        (&peer_pending, "pending", 1),
        (&peer_approved, "approved", 2),
    ];
    assert_eq!(
        baseline.keys().map(String::as_str).collect::<BTreeSet<_>>(),
        known
            .iter()
            .map(|(receipt, _, _)| receipt["correction_id"].as_str().unwrap())
            .collect::<BTreeSet<_>>()
    );
    for (receipt, status, revision) in known {
        let id = receipt["correction_id"].as_str().unwrap();
        let created_at = baseline[id]["created_at"].as_i64().unwrap();
        assert!(created_at > 0);
        assert_eq!(
            baseline[id],
            json!({
                "correction_id": id,
                "left": owned[0].0["left"],
                "right": owned[0].0["right"],
                "relation": "same_recording",
                "status": status,
                "revision": revision,
                "created_at": created_at
            })
        );
    }
    let peer_ids = [
        peer_pending["correction_id"].as_str().unwrap(),
        peer_approved["correction_id"].as_str().unwrap(),
    ];
    let peer_current: BTreeMap<String, Value> = baseline
        .into_iter()
        .filter(|(id, _)| peer_ids.contains(&id.as_str()))
        .collect();
    assert_eq!(peer_current.len(), 2);

    let held = suggestion();
    private.push(held["submission_id"].as_str().unwrap().to_owned());
    let held_bytes = held.to_string();
    let mut pending = PendingRequest::start(host.address, "/v1/corrections", token, &held_bytes);
    assert_eq!(
        request(
            &host,
            "DELETE",
            &format!(
                "/v1/admin/reporters/{}",
                reporter["reporter_id"].as_str().unwrap()
            ),
            Some(ADMIN),
            ""
        ),
        (204, Value::Null)
    );
    assert_eq!(current(&host), peer_current);
    let (status, body) = pending.finish(&held_bytes).unwrap();
    assert_eq!(status, 401);
    assert_eq!(
        serde_json::from_slice::<Value>(&body).unwrap(),
        json!({"error": "unauthorized"})
    );
    assert_eq!(current(&host), peer_current);
    assert_eq!(
        request(&host, "POST", "/v1/corrections", Some(token), "{"),
        (401, json!({"error": "unauthorized"}))
    );
    for (claim, receipt) in &owned {
        assert_eq!(
            post(&host, "/v1/corrections", token, claim),
            (401, json!({"error": "unauthorized"}))
        );
        assert_eq!(
            post(&host, &review_path(receipt), ADMIN, &review(1, "approve")),
            (404, json!({"error": "not_found"}))
        );
    }
    for (path, request, receipt) in &historical {
        assert_eq!(post(&host, path, ADMIN, request), (200, receipt.clone()));
        let mut changed = request.clone();
        changed["expected_revision"] = json!(2);
        assert_eq!(
            post(&host, path, ADMIN, &changed),
            (409, json!({"error": "submission_conflict"}))
        );
    }
    assert_eq!(
        post(&host, "/v1/corrections", peer_token, &owned[0].0),
        (200, peer_pending.clone())
    );
    assert_eq!(
        post(&host, "/v1/corrections", peer_token, &owned[1].0),
        (200, peer_approved.clone())
    );
    assert_eq!(
        post(&host, &peer_path, ADMIN, &peer_review),
        (200, peer_ack.clone())
    );
    assert_eq!(current(&host), peer_current);
    let next_peer_review = review(2, "approve");
    let (status, next_peer_ack) = post(&host, &peer_path, ADMIN, &next_peer_review);
    assert_eq!(status, 201);
    assert_eq!(next_peer_ack["revision"], 3);
    let mut final_current = peer_current;
    final_current.get_mut(peer_ids[1]).unwrap()["revision"] = json!(3);
    assert_eq!(current(&host), final_current);
    private.push(next_peer_review["request_id"].as_str().unwrap().to_owned());
    assert_eq!(
        post(&host, "/v1/outcomes", peer_token, &outcome),
        (200, peer_outcome.clone())
    );
    assert_eq!(rank(&host), 1);
    assert_eq!(current(&host), final_current);
    host.signal_term();
    host.wait_event("community_shutdown_requested");
    assert!(host.wait_exit().success());
    check_logs(&host, &private);

    let mut restarted = Host::start(fixture.path(), &state, &key, "correction-restarted");
    restarted.ready();
    assert_eq!(current(&restarted), final_current);
    assert_eq!(
        post(&restarted, "/v1/corrections", token, &held),
        (401, json!({"error": "unauthorized"}))
    );
    for (claim, receipt) in &owned {
        assert_eq!(
            post(&restarted, "/v1/corrections", token, claim),
            (401, json!({"error": "unauthorized"}))
        );
        assert_eq!(
            post(
                &restarted,
                &review_path(receipt),
                ADMIN,
                &review(1, "approve")
            ),
            (404, json!({"error": "not_found"}))
        );
    }
    for (path, request, receipt) in &historical {
        assert_eq!(
            post(&restarted, path, ADMIN, request),
            (200, receipt.clone())
        );
    }
    assert_eq!(
        post(&restarted, "/v1/corrections", peer_token, &owned[0].0),
        (200, peer_pending)
    );
    assert_eq!(
        post(&restarted, "/v1/corrections", peer_token, &owned[1].0),
        (200, peer_approved)
    );
    assert_eq!(
        post(&restarted, &peer_path, ADMIN, &peer_review),
        (200, peer_ack)
    );
    assert_eq!(
        post(&restarted, &peer_path, ADMIN, &next_peer_review),
        (200, next_peer_ack)
    );
    assert_eq!(
        post(&restarted, "/v1/outcomes", peer_token, &outcome),
        (200, peer_outcome)
    );
    assert_eq!(rank(&restarted), 1);
    assert_eq!(current(&restarted), final_current);
    restarted.signal_term();
    restarted.wait_event("community_shutdown_requested");
    assert!(restarted.wait_exit().success());
    check_logs(&restarted, &private);
    assert!(
        std::fs::read(&key).unwrap() == original_bytes,
        "Private administrator bytes changed"
    );
    let retained = key.metadata().unwrap();
    assert_eq!(
        (
            retained.dev(),
            retained.ino(),
            retained.mode(),
            retained.uid(),
            retained.gid(),
            retained.nlink(),
            retained.len(),
            retained.mtime(),
            retained.mtime_nsec()
        ),
        (
            original_metadata.dev(),
            original_metadata.ino(),
            original_metadata.mode(),
            original_metadata.uid(),
            original_metadata.gid(),
            original_metadata.nlink(),
            original_metadata.len(),
            original_metadata.mtime(),
            original_metadata.mtime_nsec()
        )
    );
}

fn suggestion() -> Value {
    json!({"submission_id": Uuid::new_v4(), "left": {"provider": "jamendo", "recording_id": "18446744073709551615"}, "right": {"provider": "youtube", "recording_id": "PrivTone001"}, "relation": "same_recording"})
}

fn review(revision: i64, decision: &str) -> Value {
    json!({"request_id": Uuid::new_v4(), "expected_revision": revision, "decision": decision})
}
fn review_path(receipt: &Value) -> String {
    format!(
        "/v1/admin/corrections/{}/review",
        receipt["correction_id"].as_str().unwrap()
    )
}

fn provision(host: &Host) -> Value {
    let (status, credential) = post(host, "/v1/admin/reporters", ADMIN, &json!({}));
    assert_eq!(status, 201);
    credential
}

fn post(host: &Host, path: &str, token: &str, body: &Value) -> (u16, Value) {
    request(host, "POST", path, Some(token), &body.to_string())
}
fn request(host: &Host, method: &str, path: &str, token: Option<&str>, body: &str) -> (u16, Value) {
    let (status, bytes) = host.request(method, path, token, body).unwrap();
    (
        status,
        if bytes.is_empty() {
            Value::Null
        } else {
            serde_json::from_slice(&bytes).unwrap()
        },
    )
}

fn rank(host: &Host) -> i64 {
    let (status, body) = request(
        host,
        "POST",
        "/v1/rankings",
        None,
        &json!({"candidates": [{"provider": "jamendo", "recording_id": "18446744073709551615"}]})
            .to_string(),
    );
    assert_eq!(status, 200);
    assert_eq!(body["ranked"].as_array().unwrap().len(), 1);
    body["ranked"][0]["successful_reporters"].as_i64().unwrap()
}

fn current(host: &Host) -> BTreeMap<String, Value> {
    let mut current = BTreeMap::new();
    for status in ["pending", "approved", "rejected"] {
        let (code, bytes) = host
            .request_exact(
                "GET",
                &format!("/v1/admin/corrections?status={status}"),
                Some(ADMIN),
                "",
            )
            .unwrap();
        assert_eq!(code, 200);
        assert!(bytes.len() <= 16_384);
        let page: Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(
            page.as_object()
                .unwrap()
                .keys()
                .map(String::as_str)
                .collect::<BTreeSet<_>>(),
            BTreeSet::from(["next_after", "suggestions"])
        );
        assert_eq!(page["next_after"], Value::Null);
        let rows = page["suggestions"].as_array().unwrap();
        assert!(rows.len() <= 32);
        for row in rows {
            assert_eq!(row["status"], status);
            let id = row["correction_id"].as_str().unwrap();
            assert_eq!(Uuid::parse_str(id).unwrap().get_version_num(), 4);
            assert!(current.insert(id.to_owned(), row.clone()).is_none());
        }
    }
    current
}

fn check_logs(host: &Host, private: &[String]) {
    let logs = host.logs();
    for forbidden in private {
        assert!(
            !logs.contains(forbidden),
            "Correction runtime logs exposed fixture input"
        );
    }
    let events: Vec<Value> = logs
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    for event in events.iter().filter(|event| {
        matches!(
            event["fields"]["event"].as_str(),
            Some(
                "community_correction_submitted"
                    | "community_correction_reviewed"
                    | "community_corrections_listed"
            )
        )
    }) {
        assert!(event["fields"]["duration_ms"].as_u64().is_some());
        if event["fields"]["event"] == "community_corrections_listed" {
            assert_eq!(event["fields"]["outcome"], "listed");
        } else {
            assert!(matches!(
                event["fields"]["outcome"].as_str(),
                Some("created" | "replayed")
            ));
        }
        let span = event["spans"]
            .as_array()
            .unwrap()
            .iter()
            .find(|span| span["name"] == "community_http_request")
            .unwrap();
        assert_eq!(
            Uuid::parse_str(span["request_id"].as_str().unwrap())
                .unwrap()
                .get_version_num(),
            4
        );
        assert!(matches!(
            span["route"].as_str(),
            Some(
                "/v1/corrections"
                    | "/v1/admin/corrections/{correction_id}/review"
                    | "/v1/admin/corrections"
            )
        ));
    }
    for name in [
        "community_correction_submitted",
        "community_correction_reviewed",
        "community_corrections_listed",
        "community_service_stopped",
        "community_state_closed",
    ] {
        assert!(events.iter().any(|event| event["fields"]["event"] == name));
    }
}
