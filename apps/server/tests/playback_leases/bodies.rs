use super::fixture::Case;
use axum::{
    http::{StatusCode, header},
    response::Response,
};
use hito_core::{PlaybackLease, test_support::hang_guard::hang_guard};
use http_body_util::BodyExt;

async fn audio_path(f: &Case) -> String {
    let response = f.begin().await;
    assert_eq!(response.status(), StatusCode::OK);
    let lease: PlaybackLease =
        serde_json::from_slice(&response.into_body().collect().await.unwrap().to_bytes()).unwrap();
    format!("/api/v1/playback/leases/{}/audio", lease.id)
}

async fn from(f: &Case, path: &str, first: usize) -> Response {
    f.request("GET", path, &[("range", &format!("bytes={first}-"))], None)
        .await
}

async fn delivered(response: Response) -> Option<Vec<u8>> {
    hang_guard("a lease audio body must settle", response.into_body().collect())
        .await
        .ok()
        .map(|collected| collected.to_bytes().to_vec())
}

#[tokio::test]
async fn a_range_request_right_after_a_cancelled_body_opens_without_a_conflict() {
    let f = hang_guard("the lease fixture must start", Case::new()).await;
    let path = audio_path(&f).await;
    let cancelled = from(&f, &path, 0).await;
    let playing = from(&f, &path, 8).await;
    let statuses = [cancelled.status(), playing.status()];
    drop(cancelled);
    let resumed = from(&f, &path, 16).await;
    let resumed_status = resumed.status();
    let resumed = delivered(resumed).await;
    let playing = delivered(playing).await;
    let original = f.original.clone();
    f.close().await;
    assert_eq!(statuses, [StatusCode::PARTIAL_CONTENT; 2]);
    assert_eq!(resumed_status, StatusCode::PARTIAL_CONTENT);
    assert_eq!(resumed.as_deref(), Some(&original[16..]));
    assert_eq!(playing.as_deref(), Some(&original[8..]));
}

#[tokio::test]
async fn a_new_range_request_replaces_the_oldest_body_while_cancelled_ones_still_close() {
    let f = hang_guard("the lease fixture must start", Case::new()).await;
    let path = audio_path(&f).await;
    let mut held = Vec::new();
    for first in [0, 8, 16, 24] {
        held.push(from(&f, &path, first).await);
    }
    let statuses: Vec<_> = held.iter().map(Response::status).collect();
    let lengths: Vec<_> = held
        .iter()
        .map(|response| response.headers().get(header::CONTENT_LENGTH).cloned())
        .collect();
    let mut bodies = Vec::new();
    for response in held {
        bodies.push(delivered(response).await);
    }
    let original = f.original.clone();
    f.close().await;
    assert_eq!(statuses, [StatusCode::PARTIAL_CONTENT; 4]);
    assert!(lengths.iter().all(Option::is_some));
    assert_eq!(
        bodies,
        [
            None,
            None,
            Some(original[16..].to_vec()),
            Some(original[24..].to_vec())
        ],
        "only the two newest bodies may stay open on one lease"
    );
}
