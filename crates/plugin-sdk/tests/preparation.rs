use hito_plugin_sdk::{
    Manifest, PreparationRequest, PreparationStatus, PreparationStatusRequest,
    PreparationStatusResponse, PreparationSubmitResponse,
};
use serde_json::json;

fn request() -> PreparationRequest {
    serde_json::from_value(json!({
        "operation_id": "54e5d994-bbe5-4160-a812-2cfd04515e7b",
        "recording_id": "opaque-recording",
        "source_id": "opaque-source"
    }))
    .unwrap()
}

#[test]
fn preparation_requires_sources_and_preserves_legacy_manifests() {
    for (capabilities, accepted) in [
        (json!(["sources"]), true),
        (json!(["sources", "source_preparation"]), true),
        (json!(["source_preparation"]), false),
    ] {
        let manifest: Manifest = serde_json::from_value(json!({
            "id": "org.hito.preparation",
            "name": "Owned preparation provider",
            "protocol_version": 1,
            "capabilities": capabilities
        }))
        .unwrap();
        assert_eq!(manifest.validate().is_ok(), accepted);
    }
}

#[test]
fn preparation_accepts_opaque_identity_and_rejects_embedded_credentials() {
    let request = request();
    assert!(request.validate().is_ok());
    let mut value = serde_json::to_value(&request).unwrap();
    value["bearer_token"] = json!("must-not-be-accepted");
    assert!(serde_json::from_value::<PreparationRequest>(value).is_err());
    let mut value = serde_json::to_value(&request).unwrap();
    value["operation_id"] = json!("00000000-0000-0000-0000-000000000000");
    assert!(
        serde_json::from_value::<PreparationRequest>(value)
            .unwrap()
            .validate()
            .is_err()
    );
}

#[test]
fn submission_acknowledgements_distinguish_accepted_rejected_and_unknown() {
    let request = request();
    for submission in [
        json!({
            "outcome": "accepted",
            "handle": "opaque-handle",
            "status": {"phase": "queued", "poll_after_seconds": 5}
        }),
        json!({
            "outcome": "not_submitted",
            "reason": "rate_limited",
            "retry_after_seconds": 30
        }),
        json!({"outcome": "submission_unknown"}),
    ] {
        let mut value = serde_json::to_value(&request).unwrap();
        value["submission"] = submission;
        let response: PreparationSubmitResponse = serde_json::from_value(value.clone()).unwrap();
        assert!(response.validate(&request).is_ok());
        value["source_id"] = json!("different-source");
        assert!(
            serde_json::from_value::<PreparationSubmitResponse>(value)
                .unwrap()
                .validate(&request)
                .is_err()
        );
    }
}

#[test]
fn preparation_progress_and_poll_delays_are_bounded() {
    for (progress, eta, delay, accepted) in [
        (None, None, 5, true),
        (Some(0), Some(0), 5, true),
        (Some(10_000), Some(604_800), 86_400, true),
        (Some(10_001), None, 5, false),
        (Some(1), Some(604_801), 5, false),
        (Some(1), None, 4, false),
        (Some(1), None, 86_401, false),
    ] {
        let status: PreparationStatus = serde_json::from_value(json!({
            "phase": "preparing",
            "progress_basis_points": progress,
            "eta_seconds": eta,
            "poll_after_seconds": delay
        }))
        .unwrap();
        assert_eq!(status.validate().is_ok(), accepted);
    }
}

#[test]
fn status_responses_are_bound_to_the_acknowledged_operation_and_handle() {
    let mut value = serde_json::to_value(request()).unwrap();
    value["handle"] = json!("opaque-handle");
    let request: PreparationStatusRequest = serde_json::from_value(value.clone()).unwrap();
    assert!(request.validate().is_ok());
    value["status"] = json!({"phase": "ready"});
    let response: PreparationStatusResponse = serde_json::from_value(value.clone()).unwrap();
    assert!(response.validate(&request).is_ok());
    for field in ["operation_id", "recording_id", "source_id", "handle"] {
        let mut changed = value.clone();
        changed[field] = if field == "operation_id" {
            json!("2ec6943f-cceb-43ea-83cd-b8b855fd90d2")
        } else {
            json!("different")
        };
        let response: PreparationStatusResponse = serde_json::from_value(changed).unwrap();
        assert!(response.validate(&request).is_err());
    }
}

#[test]
fn status_rejects_unstructured_errors_and_oversized_handles() {
    assert!(
        serde_json::from_value::<PreparationStatus>(json!({
            "phase": "failed", "reason": "raw provider error with private data"
        }))
        .is_err()
    );
    assert!(
        serde_json::from_value::<PreparationStatus>(json!({
            "phase": "ready", "download_url": "https://private.invalid/"
        }))
        .is_err()
    );
    let mut value = serde_json::to_value(request()).unwrap();
    value["handle"] = json!("x".repeat(2049));
    assert!(
        serde_json::from_value::<PreparationStatusRequest>(value)
            .unwrap()
            .validate()
            .is_err()
    );
}
