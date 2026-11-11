use super::fixture::Case;
use axum::http::StatusCode;
use hito_core::{Credentials, PlaybackLease};
use http_body_util::BodyExt;
use serde_json::json;

const ROOT: &str = "/api/v1/playback/leases";

#[tokio::test]
async fn lease_json_uses_strict_bounded_core_wire_types_and_safe_errors() {
    let f = Case::new().await;
    let mut evidence = Vec::new();
    for (kind, body, expected) in [
        ("application/json", "{".into(), StatusCode::BAD_REQUEST),
        (
            "application/json",
            "null".into(),
            StatusCode::UNPROCESSABLE_ENTITY,
        ),
        (
            "application/json",
            "{}".into(),
            StatusCode::UNPROCESSABLE_ENTITY,
        ),
        (
            "application/json",
            json!({"recording_id":f.recording,"source_id":f.source,"private-input-sentinel":true})
                .to_string(),
            StatusCode::UNPROCESSABLE_ENTITY,
        ),
        (
            "application/json",
            json!({"recording_id":f.recording,"source_id":42}).to_string(),
            StatusCode::UNPROCESSABLE_ENTITY,
        ),
        (
            "text/plain",
            json!({"recording_id":f.recording,"source_id":f.source}).to_string(),
            StatusCode::UNSUPPORTED_MEDIA_TYPE,
        ),
        (
            "application/json",
            json!({"recording_id":"private-input-sentinel".repeat(20_000),"source_id":f.source})
                .to_string(),
            StatusCode::PAYLOAD_TOO_LARGE,
        ),
    ] {
        let response = f
            .request(
                "POST",
                ROOT,
                &[("content-type", kind)],
                Some(body.into_bytes()),
            )
            .await;
        let status = response.status();
        let private = response.headers()["cache-control"] == "private, no-store";
        let bytes = response.into_body().collect().await.unwrap().to_bytes();
        let safe = !String::from_utf8_lossy(&bytes).contains("private-input-sentinel");
        evidence.push((status, expected, private, safe));
    }
    let response = f.begin().await;
    let valid = response.status();
    let wire: serde_json::Value =
        serde_json::from_slice(&response.into_body().collect().await.unwrap().to_bytes()).unwrap();
    let typed: PlaybackLease = serde_json::from_value(wire.clone()).unwrap();
    let exact_fields = wire.as_object().unwrap().len() == 2 && typed.expires_at > 0;
    f.close().await;
    assert!(
        evidence
            .iter()
            .all(|(actual, expected, private, safe)| actual == expected && *private && *safe),
        "{evidence:?}"
    );
    assert_eq!(valid, StatusCode::OK);
    assert!(exact_fields);
}

#[tokio::test]
async fn malformed_lease_paths_use_static_json_without_echoing_input() {
    let f = Case::new().await;
    let mut evidence = Vec::new();
    for (method, suffix) in [
        ("GET", ""),
        ("DELETE", ""),
        ("GET", "/audio"),
        ("HEAD", "/audio"),
    ] {
        for id in ["private-path-sentinel", "%FF"] {
            let response = f
                .request(method, &format!("{ROOT}/{id}{suffix}"), &[], None)
                .await;
            let status = response.status();
            let bytes = response.into_body().collect().await.unwrap().to_bytes();
            let safe = if method == "HEAD" {
                bytes.is_empty()
            } else {
                serde_json::from_slice::<serde_json::Value>(&bytes).unwrap()
                    == json!({"error":"Playback lease path does not match the expected fields"})
            };
            evidence.push((method, status, safe));
        }
    }
    f.close().await;
    assert!(
        evidence
            .iter()
            .all(|(_, status, safe)| *status == StatusCode::UNPROCESSABLE_ENTITY && *safe),
        "{evidence:?}"
    );
}

#[tokio::test]
async fn shared_authentication_account_and_origin_fences_apply_to_every_lease_route() {
    let f = Case::new().await;
    let response = f.begin().await;
    let lease: PlaybackLease =
        serde_json::from_slice(&response.into_body().collect().await.unwrap().to_bytes()).unwrap();
    let path = format!("{ROOT}/{}", lease.id);
    let audio = format!("{path}/audio");
    let credentials = || Credentials {
        username: "other-listener".into(),
        password: "other lease account password".into(),
    };
    f.services
        .identity
        .create_user(&f.actor, credentials())
        .await
        .unwrap();
    let other = f.services.identity.login(credentials()).await.unwrap();
    let mut evidence = Vec::new();
    for (method, target) in [
        ("POST", ROOT),
        ("GET", path.as_str()),
        ("DELETE", path.as_str()),
        ("GET", audio.as_str()),
        ("HEAD", audio.as_str()),
    ] {
        let unauthenticated = f.request_as(None, method, target, &[], None).await.status();
        let mismatched = f
            .request(
                method,
                target,
                &[("x-hito-account", "other-private-account")],
                None,
            )
            .await
            .status();
        if method != "POST" {
            let foreign = f
                .request_as(Some(&other.token), method, target, &[], None)
                .await
                .status();
            evidence.push((foreign, StatusCode::NOT_FOUND));
        }
        evidence.push((unauthenticated, StatusCode::UNAUTHORIZED));
        evidence.push((mismatched, StatusCode::FORBIDDEN));
    }
    for (method, target) in [("POST", ROOT), ("DELETE", path.as_str())] {
        evidence.push((
            f.request(
                method,
                target,
                &[("origin", "https://untrusted.invalid")],
                None,
            )
            .await
            .status(),
            StatusCode::FORBIDDEN,
        ));
    }
    let owner = f.request("GET", &path, &[], None).await.status();
    f.close().await;
    assert!(
        evidence.iter().all(|(actual, expected)| actual == expected),
        "{evidence:?}"
    );
    assert_eq!(owner, StatusCode::OK);
}
