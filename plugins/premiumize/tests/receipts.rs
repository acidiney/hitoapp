#![cfg(feature = "test-support")]

#[path = "qualification/mod.rs"]
pub mod qualification;

use hito_debrid_premiumize::TransferFileBinding;
use hito_plugin_premiumize::Config;
use hito_plugin_sdk::{
    PreparationRequest, PreparationStatus, PreparationStatusRequest, PreparationStatusResponse,
    PreparationSubmission, PreparationSubmitResponse,
};
use qualification::{Case, copy, headers, provider::*, run, sibling, transfer};
use reqwest::{Method, StatusCode};
use serde_json::Value;
use uuid::Uuid;

fn config(case: &Case) -> Config {
    let mut config = case.config();
    let extra = sibling(&config.connections[0].recordings[0]);
    config.connections[0].recordings.push(extra);
    config.connections[1].recordings = config.connections[0].recordings.iter().map(copy).collect();
    config
}

fn preparation(operation_id: Uuid) -> PreparationRequest {
    PreparationRequest {
        operation_id,
        recording_id: RECORDING.into(),
        source_id: SOURCE.into(),
    }
}

fn status(request: &PreparationRequest, handle: &str) -> PreparationStatusRequest {
    PreparationStatusRequest {
        operation_id: request.operation_id,
        recording_id: request.recording_id.clone(),
        source_id: request.source_id.clone(),
        handle: handle.into(),
    }
}

fn accepted(value: Value, request: &PreparationRequest) -> String {
    let response: PreparationSubmitResponse = serde_json::from_value(value).unwrap();
    response.validate(request).unwrap();
    match response.submission {
        PreparationSubmission::Accepted {
            handle,
            status: PreparationStatus::Queued {
                poll_after_seconds: 5,
            },
        } => handle,
        _ => panic!("only the freshly observed queued transfer may be accepted"),
    }
}

fn private_error(value: &Value, operation: Uuid, handle: &str) {
    assert!(value.get("error").is_some());
    let text = value.to_string();
    assert!(
        !text.contains(handle),
        "an error disclosed the original receipt"
    );
    assert!(
        !text.contains(&operation.to_string()),
        "an error disclosed the original operation"
    );
}

#[tokio::test]
async fn the_same_authority_rebuilds_the_same_receipt_after_router_client_and_catalog_reordering() {
    run(
        vec![
            transfer("queued", 0.0),
            transfer("queued", 0.0),
            transfer("running", 0.5),
        ],
        |case| {
            Box::pin(async move {
                let request = preparation(Uuid::from_u128(0x7171));
                case.protect(&request.operation_id.to_string());
                let body = serde_json::to_string(&request).unwrap();
                let first = case.start(config(case)).await;
                let response = case
                    .request(
                        &first,
                        Method::POST,
                        "v1/sources/prepare",
                        headers(Some(SERVICE)),
                        Some(&body),
                        StatusCode::OK,
                    )
                    .await;
                let handle = accepted(response, &request);
                assert!(!handle.contains(&request.operation_id.to_string()));
                case.protect(&handle);
                assert_eq!(case.counts(), [1, 0, 0, 0]);
                case.stop().await;

                let mut reordered = config(case);
                reordered.connections.reverse();
                for connection in &mut reordered.connections {
                    connection.recordings.reverse();
                }
                let restarted = case.start(reordered).await;
                let response = case
                    .request(
                        &restarted,
                        Method::POST,
                        "v1/sources/prepare",
                        headers(Some(SERVICE)),
                        Some(&body),
                        StatusCode::OK,
                    )
                    .await;
                let rebuilt = accepted(response, &request);
                assert!(
                    rebuilt == handle,
                    "trusted configuration reordering changed the receipt"
                );
                assert_eq!(case.counts(), [2, 0, 0, 0]);

                let poll = status(&request, &handle);
                let body = serde_json::to_string(&poll).unwrap();
                let value = case
                    .request(
                        &restarted,
                        Method::POST,
                        "v1/sources/preparation/status",
                        headers(Some(SERVICE)),
                        Some(&body),
                        StatusCode::OK,
                    )
                    .await;
                let observed: PreparationStatusResponse = serde_json::from_value(value).unwrap();
                observed.validate(&poll).unwrap();
                assert!(matches!(
                    observed.status,
                    PreparationStatus::Preparing {
                        progress_basis_points: Some(5000),
                        eta_seconds: None,
                        poll_after_seconds: 5,
                    }
                ));
                assert_eq!(
                    case.counts(),
                    [3, 0, 0, 0],
                    "the current phase must be observed after restart"
                );
            })
        },
    )
    .await;
}

#[tokio::test]
async fn altered_receipts_authorities_and_every_bound_file_field_are_rejected_before_provider_io() {
    run(
        vec![transfer("queued", 0.0), transfer("running", 0.375)],
        |case| {
            Box::pin(async move {
                let request = preparation(Uuid::from_u128(0x7272));
                case.protect(&request.operation_id.to_string());
                let base = case.start(config(case)).await;
                let body = serde_json::to_string(&request).unwrap();
                let response = case
                    .request(
                        &base,
                        Method::POST,
                        "v1/sources/prepare",
                        headers(Some(SERVICE)),
                        Some(&body),
                        StatusCode::OK,
                    )
                    .await;
                let handle = accepted(response, &request);
                case.protect(&handle);
                let poll = status(&request, &handle);
                let valid = serde_json::to_value(&poll).unwrap();
                let mut tampered = handle.as_bytes().to_vec();
                let middle = tampered.len() / 2;
                tampered[middle] = if tampered[middle] == b'A' { b'B' } else { b'A' };
                let forged_handle = String::from_utf8(tampered).unwrap();
                case.protect(&forged_handle);
                let mut forged = valid.clone();
                forged["handle"] = forged_handle.into();
                let mut wrong_operation = valid.clone();
                case.protect(&Uuid::from_u128(0x7373).to_string());
                wrong_operation["operation_id"] = Uuid::from_u128(0x7373).to_string().into();
                let mut wrong_source = valid.clone();
                wrong_source["source_id"] = "owned-private-second-source".into();
                let mut other_entry = wrong_source.clone();
                other_entry["recording_id"] = "owned-private-second-recording".into();
                for (token, payload) in [
                    (SERVICE, forged),
                    (SERVICE, wrong_operation),
                    (SERVICE, wrong_source),
                    (SERVICE, other_entry),
                    (PEER_SERVICE, valid.clone()),
                ] {
                    let before = case.counts();
                    let body = payload.to_string();
                    let error = case
                        .request(
                            &base,
                            Method::POST,
                            "v1/sources/preparation/status",
                            headers(Some(token)),
                            Some(&body),
                            StatusCode::NOT_FOUND,
                        )
                        .await;
                    private_error(&error, request.operation_id, &handle);
                    assert_eq!(
                        case.counts(),
                        before,
                        "a rejected replay performed provider I/O"
                    );
                }

                type Mutation = fn(&mut TransferFileBinding);
                let mutations: &[Mutation] = &[
                    |binding| binding.transfer.transfer_id.push_str("-replacement"),
                    |binding| binding.transfer.created_at_unix_seconds += 1,
                    |binding| binding.item_id.push_str("-replacement"),
                    |binding| binding.item_created_at_unix_seconds += 1,
                    |binding| binding.file_name.push_str(".replacement.wav"),
                    |binding| binding.file_bytes += 2,
                    |binding| binding.folder_id.push_str("-replacement"),
                ];
                let body = valid.to_string();
                for mutate in mutations {
                    let mut changed = config(case);
                    mutate(&mut changed.connections[0].recordings[0].binding);
                    let restarted = case.start(changed).await;
                    let before = case.counts();
                    let error = case
                        .request(
                            &restarted,
                            Method::POST,
                            "v1/sources/preparation/status",
                            headers(Some(SERVICE)),
                            Some(&body),
                            StatusCode::NOT_FOUND,
                        )
                        .await;
                    private_error(&error, request.operation_id, &handle);
                    assert_eq!(
                        case.counts(),
                        before,
                        "changed binding authority performed provider I/O"
                    );
                    case.stop().await;
                }
                let mut rotated = config(case);
                rotated.connections[0].receipt_key = [0x73; 32];
                let restarted = case.start(rotated).await;
                let before = case.counts();
                let error = case
                    .request(
                        &restarted,
                        Method::POST,
                        "v1/sources/preparation/status",
                        headers(Some(SERVICE)),
                        Some(&body),
                        StatusCode::NOT_FOUND,
                    )
                    .await;
                private_error(&error, request.operation_id, &handle);
                assert_eq!(
                    case.counts(),
                    before,
                    "a rotated account receipt performed provider I/O"
                );
                case.stop().await;

                let mut changed_service = config(case);
                let rotated_bearer = "owned-private-replaced-service-authority";
                changed_service.connections[0].service_bearer = rotated_bearer.into();
                let restarted = case.start(changed_service).await;
                let before = case.counts();
                let error = case
                    .request(
                        &restarted,
                        Method::POST,
                        "v1/sources/preparation/status",
                        headers(Some(rotated_bearer)),
                        Some(&body),
                        StatusCode::NOT_FOUND,
                    )
                    .await;
                private_error(&error, request.operation_id, &handle);
                assert_eq!(
                    case.counts(),
                    before,
                    "a changed service authority performed provider I/O"
                );
                case.stop().await;

                let value = case
                    .request(
                        &base,
                        Method::POST,
                        "v1/sources/preparation/status",
                        headers(Some(SERVICE)),
                        Some(&body),
                        StatusCode::OK,
                    )
                    .await;
                let observed: PreparationStatusResponse = serde_json::from_value(value).unwrap();
                observed.validate(&poll).unwrap();
                assert!(matches!(
                    observed.status,
                    PreparationStatus::Preparing {
                        progress_basis_points: Some(3750),
                        eta_seconds: None,
                        poll_after_seconds: 5,
                    }
                ));
                assert_eq!(
                    case.counts(),
                    [2, 0, 0, 0],
                    "only legitimate current observations may reach the provider"
                );
            })
        },
    )
    .await;
}
