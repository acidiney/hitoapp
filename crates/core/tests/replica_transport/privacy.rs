use super::{Credentials, Endpoint, Remote, TOKEN, raw::RawServer};
use hito_testkit::logs::LogCapture;
use std::{sync::OnceLock, time::Duration};
use tracing::instrument::WithSubscriber;

static GLOBAL: OnceLock<LogCapture> = OnceLock::new();

fn global_capture() -> LogCapture {
    GLOBAL
        .get_or_init(|| {
            let capture = LogCapture::default();
            tracing::dispatcher::set_global_default(capture.dispatch_with_spans("trace")).unwrap();
            capture
        })
        .clone()
}

async fn private_exchanges(capture: &LogCapture) {
    let valid_body=serde_json::to_vec(&serde_json::json!({"token":"private-issued-token","user":super::fixture::account("private-account","private-user")})).unwrap();
    let response = |length: usize| {
        let mut response = format!(
            "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {length}\r\n\r\n"
        )
        .into_bytes();
        response.extend_from_slice(&valid_body);
        response
    };
    let successful = RawServer::start(response(valid_body.len()), false).await;
    let successful_endpoint = successful.endpoint.clone();
    let remote = Remote::new(Endpoint::parse(&successful.endpoint).unwrap()).unwrap();
    assert_eq!(
        remote
            .login(Credentials {
                username: "private-user".into(),
                password: "private-password".into()
            })
            .await
            .unwrap()
            .token,
        "private-issued-token"
    );
    successful.close().await;
    let server = RawServer::start(response(valid_body.len() + 1), true).await;
    let endpoint = server.endpoint.clone();
    let remote = Remote::with_deadlines(
        Endpoint::parse(&endpoint).unwrap(),
        Duration::from_millis(150),
        Duration::from_millis(100),
    )
    .unwrap();
    assert!(
        remote
            .login(Credentials {
                username: "private-user".into(),
                password: "private-password".into()
            })
            .await
            .is_err()
    );
    server.close().await;

    let held = RawServer::start(Vec::new(), true).await;
    let held_endpoint = held.endpoint.clone();
    let remote = Remote::with_deadlines(
        Endpoint::parse(&held.endpoint).unwrap(),
        Duration::from_secs(2),
        Duration::from_secs(2),
    )
    .unwrap();
    let dispatch = tracing::dispatcher::get_default(Clone::clone);
    let request = tokio::spawn(
        async move { remote.me(TOKEN, "private-account").await }.with_subscriber(dispatch),
    );
    held.received().await;
    request.abort();
    assert!(request.await.unwrap_err().is_cancelled());
    held.close().await;
    tokio::task::yield_now().await;
    private_catalog_exchanges(capture).await;
    capture.assert_excludes(&[
        &successful_endpoint,
        &endpoint,
        &held_endpoint,
        "localhost",
        "127.0.0.1",
        "private-issued-token",
        "private-account",
        "private-user",
        "private-password",
        TOKEN,
    ]);
    let events = capture.events();
    for outcome in ["complete", "unavailable", "cancelled"] {
        assert!(
            events.iter().any(
                |event| event["fields"]["event"] == "replica_exchange_finished"
                    && event["fields"]["outcome"] == outcome
                    && event["fields"]["elapsed_ms"].is_u64()
                    && event["span"]["request_id"]
                        .as_str()
                        .is_some_and(|id| uuid::Uuid::parse_str(id).is_ok())
            ),
            "safe correlated transport outcome was lost: {outcome}"
        );
    }
}

async fn private_catalog_exchanges(capture: &LogCapture) {
    use super::catalog::{Call, SNAPSHOT};
    let metadata = serde_json::json!({
        "id": SNAPSHOT, "account_id": "owned-account", "library_id": "private-library",
        "mode": "snapshot_only", "collection_watermark": 7, "recording_count": 1,
        "page_count": 1, "expires_at": 1800000000
    });
    let page = serde_json::json!({
        "snapshot_id": SNAPSHOT, "ordinal": 0, "next_ordinal": null,
        "recordings": [{"id":"private-recording", "library_id":"private-library",
            "title":"private-title", "artist":"private-artist", "duration_ms":1, "sources":[]}]
    });
    for (call, operation, value) in [
        (Call::Begin, "begin_catalog_snapshot", Some(metadata)),
        (Call::Page, "catalog_snapshot_page", Some(page)),
        (Call::Release, "release_catalog_snapshot", None),
    ] {
        let response = match value {
            Some(value) => {
                let bytes = serde_json::to_vec(&value).unwrap();
                let mut response = format!("HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\n\r\n", bytes.len()).into_bytes();
                response.extend_from_slice(&bytes);
                response
            }
            None => b"HTTP/1.1 204 No Content\r\n\r\n".to_vec(),
        };
        let server = RawServer::start(response, false).await;
        let endpoint = server.endpoint.clone();
        let remote = Remote::new(Endpoint::parse(&endpoint).unwrap()).unwrap();
        call.run(&remote).await.unwrap();
        server.close().await;
        let held = RawServer::start(Vec::new(), true).await;
        let held_endpoint = held.endpoint.clone();
        let remote = Remote::new(Endpoint::parse(&held_endpoint).unwrap()).unwrap();
        let dispatch = tracing::dispatcher::get_default(Clone::clone);
        let request =
            tokio::spawn(async move { call.run(&remote).await }.with_subscriber(dispatch));
        held.received().await;
        request.abort();
        assert!(request.await.unwrap_err().is_cancelled());
        held.close().await;
        capture.assert_excludes(&[&endpoint, &held_endpoint]);
        for outcome in ["complete", "cancelled"] {
            assert!(
                capture.events().iter().any(|event| event["fields"]["event"]
                    == "replica_exchange_finished"
                    && event["fields"]["outcome"] == outcome
                    && event["fields"]["elapsed_ms"].is_u64()
                    && event["span"]["operation"] == operation
                    && event["span"]["request_id"]
                        .as_str()
                        .is_some_and(|id| uuid::Uuid::parse_str(id).is_ok())),
                "missing correlated catalog outcome: {operation} {outcome}"
            );
        }
    }
    capture.assert_excludes(&[
        SNAPSHOT,
        "owned-account",
        "private-library",
        "private-recording",
        "private-title",
        "private-artist",
    ]);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn global_trace_keeps_safe_workflow_context_and_excludes_vendor_private_data() {
    const CHILD: &str = "HITO_REPLICA_GLOBAL_TRACE_CHILD";
    if std::env::var_os(CHILD).is_none() {
        let result=std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact","privacy::global_trace_keeps_safe_workflow_context_and_excludes_vendor_private_data","--nocapture"])
            .env(CHILD,"1").output().unwrap();
        assert!(
            result.status.success(),
            "global TRACE child failed: {}",
            String::from_utf8_lossy(&result.stdout)
        );
        return;
    }
    private_exchanges(&global_capture()).await;
}

#[tokio::test]
async fn scoped_trace_keeps_safe_workflow_context_and_excludes_vendor_private_data() {
    let capture = LogCapture::default();
    private_exchanges(&capture)
        .with_subscriber(capture.dispatch_with_spans("trace"))
        .await;
}
