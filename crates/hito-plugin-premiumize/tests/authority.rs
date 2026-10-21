#![cfg(feature = "test-support")]

#[path = "qualification/mod.rs"]
pub mod qualification;

use hito_debrid_premiumize::MediaAllowlist;
use hito_plugin_premiumize::{ConnectionConfig, router};
use qualification::{headers, provider::*, run, sibling, transfer};
use reqwest::{Method, StatusCode, header::HeaderValue};
use serde_json::json;

#[tokio::test]
async fn connection_authority_precedes_payload_parsing_and_never_crosses_catalogs() {
    run(vec![transfer("queued", 0.0)], |case| {
        Box::pin(async move {
            let base = case.start(case.config()).await;
            let mut duplicate = headers(Some(SERVICE));
            duplicate.append(
                "authorization",
                HeaderValue::from_str(&format!("Bearer {PEER_SERVICE}")).unwrap(),
            );
            let mut duplicate_protocol = headers(Some(SERVICE));
            duplicate_protocol.append("x-hito-plugin-protocol", HeaderValue::from_static("1"));
            let mut wrong_protocol = headers(Some(SERVICE));
            wrong_protocol.insert("x-hito-plugin-protocol", HeaderValue::from_static("2"));
            let mut missing_protocol = headers(Some(SERVICE));
            missing_protocol.remove("x-hito-plugin-protocol");
            for (method, path, auth, expected) in [
                (
                    Method::POST,
                    "v1/search",
                    headers(None),
                    StatusCode::UNAUTHORIZED,
                ),
                (
                    Method::POST,
                    "v1/sources/prepare",
                    headers(Some(WRONG_SERVICE)),
                    StatusCode::UNAUTHORIZED,
                ),
                (
                    Method::GET,
                    "v1/audio?source_id=one&source_id=two",
                    headers(None),
                    StatusCode::UNAUTHORIZED,
                ),
                (
                    Method::GET,
                    "v1/audio?source_id=one&source_id=two",
                    headers(Some(WRONG_SERVICE)),
                    StatusCode::UNAUTHORIZED,
                ),
                (
                    Method::POST,
                    "v1/search",
                    duplicate,
                    StatusCode::UNAUTHORIZED,
                ),
                (
                    Method::POST,
                    "v1/search",
                    duplicate_protocol,
                    StatusCode::BAD_REQUEST,
                ),
                (
                    Method::POST,
                    "v1/search",
                    wrong_protocol,
                    StatusCode::BAD_REQUEST,
                ),
                (
                    Method::POST,
                    "v1/search",
                    missing_protocol,
                    StatusCode::BAD_REQUEST,
                ),
            ] {
                let response = case
                    .request(&base, method, path, auth, Some("{"), expected)
                    .await;
                assert!(response.get("error").is_some());
                assert_eq!(case.counts(), [0, 0, 0, 0]);
            }
            for (token, query, expected) in [
                (SERVICE, QUERY, RECORDING),
                (PEER_SERVICE, "owned-private-peer", PEER_RECORDING),
            ] {
                let body = json!({"query":query, "cursor":null, "limit":10}).to_string();
                let page = case
                    .request(
                        &base,
                        Method::POST,
                        "v1/search",
                        headers(Some(token)),
                        Some(&body),
                        StatusCode::OK,
                    )
                    .await;
                assert_eq!(page["items"].as_array().unwrap().len(), 1);
                assert!(page["items"][0]["id"] == expected);
            }
            for (token, recording) in [(SERVICE, PEER_RECORDING), (PEER_SERVICE, RECORDING)] {
                let body = json!({"recording_id":recording}).to_string();
                case.request(
                    &base,
                    Method::POST,
                    "v1/recordings/detail",
                    headers(Some(token)),
                    Some(&body),
                    StatusCode::NOT_FOUND,
                )
                .await;
            }
            assert_eq!(case.counts(), [0, 0, 0, 0]);
        })
    })
    .await;
}

#[tokio::test]
async fn ambiguous_or_invalid_trusted_configuration_fails_before_provider_io() {
    run(vec![transfer("queued", 0.0)], |case| {
        Box::pin(async move {
            type Mutation = fn(&mut Vec<ConnectionConfig>);
            let mutations: &[Mutation] = &[
                |connections| connections.clear(),
                |connections| connections[0].service_bearer.clear(),
                |connections| connections[0].service_bearer.push('\n'),
                |connections| connections[1].service_bearer = connections[0].service_bearer.clone(),
                |connections| connections[0].receipt_key = [0; 32],
                |connections| connections[1].receipt_key = connections[0].receipt_key,
                |connections| connections[0].recordings.clear(),
                |connections| connections[0].recordings[0].recording.title.clear(),
                |connections| connections[0].recordings[0].source_id.clear(),
                |connections| connections[0].recordings[0].binding.file_bytes = 43,
                |connections| {
                    let mut extra = sibling(&connections[0].recordings[0]);
                    extra.recording.id = connections[0].recordings[0].recording.id.clone();
                    connections[0].recordings.push(extra);
                },
                |connections| {
                    let mut extra = sibling(&connections[0].recordings[0]);
                    extra.source_id = connections[0].recordings[0].source_id.clone();
                    connections[0].recordings.push(extra);
                },
                |connections| {
                    let mut extra = sibling(&connections[0].recordings[0]);
                    extra.binding.transfer = connections[0].recordings[0].binding.transfer.clone();
                    connections[0].recordings.push(extra);
                },
                |connections| {
                    let mut extra = sibling(&connections[0].recordings[0]);
                    extra.binding.item_id = connections[0].recordings[0].binding.item_id.clone();
                    connections[0].recordings.push(extra);
                },
            ];
            for mutate in mutations {
                let mut config = case.config();
                mutate(&mut config.connections);
                assert!(
                    router(config).is_err(),
                    "an invalid configuration initialized"
                );
                assert_eq!(case.counts(), [0, 0, 0, 0]);
            }
            for origin in [
                "http://media.hito-fixture.invalid",
                "https://127.0.0.1",
                "https://user@media.hito-fixture.invalid",
                "https://media.hito-fixture.invalid/private",
                "https://media.hito-fixture.invalid/?token=owned-private-origin",
            ] {
                assert!(MediaAllowlist::new(vec![origin.into()]).is_err());
            }
            assert_eq!(case.counts(), [0, 0, 0, 0]);
        })
    })
    .await;
}
