use super::{choice_http::call, fixture::Provider, http};
use axum::http::StatusCode;
use serde_json::{Value, json};
use std::net::SocketAddr;
use uuid::Uuid;

pub(super) struct SavedAccount {
    pub token: String,
    pub recording: String,
    pub source: String,
    pub choice_path: String,
    pub reset_path: String,
    metadata: Value,
}

impl SavedAccount {
    pub(super) async fn adopt(
        address: SocketAddr,
        session: &Value,
        service: &Value,
        provider: &Provider,
    ) -> Self {
        let token = session["token"]
            .as_str()
            .unwrap_or_else(|| panic!("the authenticated account must have a bearer token"));
        let installation = call(
            address,
            "POST",
            "/api/v1/plugins",
            json!({"service_id": service["id"], "service_revision": service["revision"]}),
            Some(token),
            StatusCode::CREATED,
        )
        .await;
        assert!(
            installation["service_id"] == service["id"],
            "both accounts must install the same offered service"
        );
        let adopted = call(address, "POST", &format!("/api/v1/plugins/{}/recordings", installation["id"].as_str().unwrap()), json!({"installation_revision": installation["revision"], "provider_recording_id": provider.recording_id()}), Some(token), StatusCode::OK).await;
        let recording = adopted["id"].as_str().unwrap().to_owned();
        let sources = http::get(
            address,
            token,
            &format!("/api/v1/recordings/{recording}/sources"),
        )
        .await;
        assert!(
            sources.as_array().unwrap().len() == 1
                && sources[0]["provider"] == "plugin"
                && sources[0]["ready"] == true
                && sources[0]["recording_id"] == recording,
            "each account must own its independent ready provider representation"
        );
        let source = sources[0]["id"].as_str().unwrap().to_owned();
        for id in [&recording, &source] {
            assert!(
                Uuid::parse_str(id).is_ok_and(|id| !id.is_nil()),
                "saved recording and source identities must be nonnil Hito UUIDs"
            );
        }
        let metadata = http::get(
            address,
            token,
            &format!("/api/v1/recordings/{recording}/metadata"),
        )
        .await;
        assert!(
            metadata["selected_observation"]["plugin_installation_id"] == installation["id"]
                && metadata["selected_observation"]["provider_recording_id"]
                    == provider.recording_id(),
            "each account must retain provenance from its own installation"
        );
        Self {
            token: token.to_owned(),
            choice_path: format!("/api/v1/recordings/{recording}/source-choice"),
            reset_path: format!("/api/v1/recordings/{recording}/source-choice/reset"),
            recording,
            source,
            metadata,
        }
    }

    pub(super) async fn snapshot(
        &self,
        address: SocketAddr,
        token: &str,
        mode: &str,
        revision: u64,
    ) -> Value {
        let state = http::get(address, token, &self.choice_path).await;
        assert!(
            state["recording_id"] == self.recording
                && state["mode"] == mode
                && state["revision"] == revision,
            "each account must retain its own current source choice and revision"
        );
        let sources = state["sources"].as_array().unwrap();
        assert!(
            sources.len() == 1
                && sources[0]["source"]["id"] == self.source
                && sources[0]["source"]["recording_id"] == self.recording
                && sources[0]["source"]["provider"] == "plugin"
                && sources[0]["source"]["ready"] == true
                && sources[0]["selectable"] == true
                && sources[0]["selected"] == (mode == "manual"),
            "a choice snapshot must contain only that account's current source facts"
        );
        let metadata = http::get(
            address,
            token,
            &format!("/api/v1/recordings/{}/metadata", self.recording),
        )
        .await;
        assert!(
            metadata == self.metadata,
            "source controls and authority checks must preserve the account's original metadata and provenance"
        );
        state
    }
}

pub(super) fn assert_reset(receipt: &Value, request_id: Uuid, recording: &str, revision: u64) {
    let mut keys: Vec<&str> = receipt
        .as_object()
        .unwrap()
        .keys()
        .map(String::as_str)
        .collect();
    keys.sort_unstable();
    assert!(
        keys == ["recording_id", "request_id", "revision"],
        "reset acknowledgements must expose only immutable Hito identity fields"
    );
    assert!(
        receipt["request_id"] == request_id.to_string()
            && receipt["recording_id"] == recording
            && receipt["revision"] == revision,
        "reset receipts must belong to the requesting account's recording and command"
    );
}

pub(super) fn assert_public_ack(receipt: &Value, provider: &Provider) {
    let encoded = receipt.to_string();
    for private in [
        provider.recording_id(),
        provider.source_id(),
        provider.url.clone(),
    ] {
        assert!(
            !encoded.contains(&private),
            "public acknowledgements must omit opaque provider identities and service addresses"
        );
    }
}
