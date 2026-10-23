use super::{
    entries, fixtures, provider,
    workflow::{Response, Saved},
};
use hito_plugin_sdk::{
    PreparationStatus, PreparationStatusResponse, PreparationSubmission, PreparationSubmitResponse,
    SourcePage,
};
use reqwest::StatusCode;
use tokio::time::Instant;

pub fn control_observation() -> Vec<u8> {
    let binding = entries::control_binding();
    serde_json::to_vec(&serde_json::json!({
        "success":true, "error":null, "detail":"owned-private-control-observation",
        "data": {"id":binding.torrent_id, "hash":binding.info_hash, "size":binding.torrent_bytes,
            "download_finished":true, "download_present":true, "download_state":"completed",
            "name":"owned-private-control-title", "files":[
                {"id":binding.file_id,"name":binding.file_name,"size":binding.file_bytes}
            ]}
    }))
    .unwrap()
}

pub fn assert_unknown_submission((status, body): Response, saved: &Saved) {
    assert_eq!(status, StatusCode::OK);
    let response: PreparationSubmitResponse = serde_json::from_slice(&body)
        .unwrap_or_else(|_| panic!("the Unknown submission must retain its typed wire shape"));
    assert!(response.validate(&saved.request).is_ok());
    assert!(
        matches!(response.submission, PreparationSubmission::Accepted {
        handle, status: PreparationStatus::SubmissionUnknown {},
    } if handle == saved.receipt.handle)
    );
}

pub fn assert_unknown_status((status, body): Response, saved: &Saved) {
    assert_eq!(status, StatusCode::OK);
    let response: PreparationStatusResponse = serde_json::from_slice(&body)
        .unwrap_or_else(|_| panic!("the Unknown receipt must retain its typed wire shape"));
    assert!(response.validate(&saved.receipt).is_ok());
    assert!(matches!(
        response.status,
        PreparationStatus::SubmissionUnknown {}
    ));
}

pub fn assert_closed_error((status, bytes): Response, expected: StatusCode, message: &str) {
    assert_eq!(status, expected);
    let body: serde_json::Value = serde_json::from_slice(&bytes)
        .unwrap_or_else(|_| panic!("a denied catalog request must retain closed JSON"));
    assert!(body == serde_json::json!({"error":message}));
}

pub fn assert_not_ready((status, bytes): Response) {
    assert_eq!(status, StatusCode::OK);
    let page: SourcePage = serde_json::from_slice(&bytes).unwrap_or_else(|_| {
        panic!("the incompatible current source must retain its typed wire shape")
    });
    assert!(page.validate().is_ok());
    assert_eq!(page.items.len(), 1);
    assert!(page.items[0].id == entries::SOURCE && !page.items[0].ready);
}

pub fn assert_uncertain_requests(requests: &[provider::Request], windows: &[(Instant, Instant)]) {
    assert_eq!(requests.len(), 4 + windows.len());
    assert_eq!(
        requests
            .iter()
            .filter(|request| request.target.starts_with("/v1/api/torrents/requestdl?"))
            .count(),
        2
    );
    let control_observation = "/v1/api/torrents/mylist?id=8124&bypass_cache=true";
    let original_observation = "/v1/api/torrents/mylist?id=7123&bypass_cache=true";
    let control_mint = "/v1/api/torrents/requestdl?token=owned-torbox-key%2F%2B%3F%3D%26-kept-private&torrent_id=8124&file_id=19&zip_link=false&redirect=false&append_name=false";
    let original_mint = "/v1/api/torrents/requestdl?token=owned-torbox-key%2F%2B%3F%3D%26-kept-private&torrent_id=7123&file_id=7&zip_link=false&redirect=false&append_name=false";
    let expected_authorization = format!("Bearer {}", fixtures::TOKEN);
    for (index, request) in requests.iter().enumerate() {
        assert!(
            request.server_name == "api.torbox.app"
                && request.method == "GET"
                && request.body.is_empty()
        );
        for name in [
            "cookie",
            "referer",
            "proxy-authorization",
            "x-hito-plugin-protocol",
        ] {
            assert!(request.header(name).is_none());
        }
        let expected = match index {
            0 => control_observation,
            1 => control_mint,
            2 => original_observation,
            3 => original_mint,
            _ => control_observation,
        };
        assert!(request.target == expected);
        if matches!(index, 1 | 3) {
            assert!(request.header("authorization").is_none());
        } else {
            assert!(request.header("authorization") == Some(expected_authorization.as_str()));
        }
        if index >= 4 {
            let (started, finished) = windows[index - 4];
            assert!((started..=finished).contains(&request.received_at));
        }
    }
}
