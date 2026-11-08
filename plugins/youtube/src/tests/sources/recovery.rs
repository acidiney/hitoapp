use super::identity::{audio, calls, executable, recording, shutdown, source};
use super::{Capture, bounded_body, exchange, https, owned_aac, private, representation};
use crate::{Config, Service};
use axum::http::{Method, StatusCode};

struct Setup {
    directory: tempfile::TempDir,
    original: Vec<u8>,
    cdn: https::Fixture,
    service: Service,
    capture: Capture,
    source: String,
}

async fn setup() -> Setup {
    let directory = tempfile::tempdir().unwrap();
    let original = owned_aac(directory.path());
    let cdn = https::Fixture::start(original.clone()).await;
    let executable = executable(directory.path());
    recording(directory.path(), "HitoTone001", original.len(), &cdn.url());
    let capture = Capture::default();
    let service = Service::with_media_client(Config { executable }, cdn.client());
    let source = source(
        &service.router(),
        &capture.dispatch(),
        "HitoTone001",
        directory.path(),
    )
    .await;
    Setup {
        directory,
        original,
        cdn,
        service,
        capture,
        source,
    }
}

impl Setup {
    async fn play(&self) -> axum::response::Response {
        exchange(
            &self.service.router(),
            &self.capture.dispatch(),
            Method::GET,
            &audio(&self.source),
            None,
            &[],
        )
        .await
    }

    async fn finish(mut self) {
        shutdown(&self.service).await;
        self.cdn.shutdown().await;
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn rejected_stale_link_is_reextracted_and_retried_once() {
    let setup = setup().await;
    setup.cdn.reject(403, 1);
    let response = setup.play().await;
    representation(&response, StatusCode::OK, setup.original.len(), None);
    assert_eq!(
        bounded_body(response).await.as_ref(),
        setup.original.as_slice()
    );
    assert_eq!(calls(setup.directory.path()).len(), 2);
    assert_eq!(setup.cdn.received().len(), 2);
    setup.finish().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn changed_validator_across_extractions_still_plays() {
    let setup = setup().await;
    let response = setup.play().await;
    assert_eq!(response.status(), StatusCode::OK);
    bounded_body(response).await;
    setup.cdn.change_etag("owned-aac-from-another-edge");
    let response = setup.play().await;
    representation(&response, StatusCode::OK, setup.original.len(), None);
    assert_eq!(
        bounded_body(response).await.as_ref(),
        setup.original.as_slice()
    );
    assert_eq!(calls(setup.directory.path()).len(), 2);
    let response = setup.play().await;
    assert_eq!(response.status(), StatusCode::OK);
    bounded_body(response).await;
    assert_eq!(calls(setup.directory.path()).len(), 2);
    setup.finish().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn persistent_rejection_retries_once_then_reports_a_bad_gateway() {
    let setup = setup().await;
    setup.cdn.reject(403, 100);
    let before = setup.cdn.received().len();
    let response = setup.play().await;
    assert_eq!(response.status(), StatusCode::BAD_GATEWAY);
    private(&bounded_body(response).await, setup.directory.path());
    assert_eq!(setup.cdn.received().len() - before, 2);
    assert_eq!(calls(setup.directory.path()).len(), 2);
    let output = setup.capture.0.text();
    assert!(output.contains("upstream_rejected"));
    assert!(output.contains("reextracted"));
    assert!(!output.contains("private-"));
    setup.finish().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn reextraction_that_changes_the_source_identity_stays_not_found() {
    let setup = setup().await;
    setup.cdn.reject(403, 1);
    let changed = setup
        .cdn
        .url()
        .replace("lmt=1730000000000000", "lmt=1730000000000009");
    recording(
        setup.directory.path(),
        "HitoTone001",
        setup.original.len(),
        &changed,
    );
    let before = setup.cdn.received().len();
    let response = setup.play().await;
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    assert_eq!(setup.cdn.received().len() - before, 1);
    setup.finish().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn rejected_range_and_head_requests_recover_the_same_way() {
    let setup = setup().await;
    setup.cdn.reject(410, 1);
    let response = exchange(
        &setup.service.router(),
        &setup.capture.dispatch(),
        Method::GET,
        &audio(&setup.source),
        Some("bytes=16-47"),
        &[],
    )
    .await;
    assert_eq!(response.status(), StatusCode::PARTIAL_CONTENT);
    assert_eq!(
        bounded_body(response).await.as_ref(),
        &setup.original[16..48]
    );
    setup.cdn.reject(404, 1);
    let response = exchange(
        &setup.service.router(),
        &setup.capture.dispatch(),
        Method::HEAD,
        &audio(&setup.source),
        None,
        &[],
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    setup.finish().await;
}
