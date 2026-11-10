use super::{BeginCatalogSnapshot, Endpoint, Remote, TOKEN, fixture::Server, json};
use serde_json::Value;
use uuid::Uuid;

pub(super) const SNAPSHOT: &str = "33333333-3333-4333-8333-333333333333";

pub(super) fn metadata() -> Value {
    json!({
        "id": SNAPSHOT, "account_id": "owned-account", "library_id": "owned-library",
        "mode": "snapshot_only", "collection_watermark": 7, "recording_count": 0,
        "page_count": 1, "expires_at": 1800000000
    })
}

#[tokio::test]
async fn begins_a_catalog_lease_with_the_account_fence_and_shared_wire_request() {
    let server = Server::start(vec![metadata()]).await;
    let remote = Remote::new(Endpoint::parse(&server.endpoint).unwrap()).unwrap();
    let snapshot = remote
        .begin_catalog_snapshot(
            TOKEN,
            "owned-account",
            &BeginCatalogSnapshot {
                library_id: "owned-library".into(),
            },
        )
        .await
        .unwrap();
    assert_eq!(snapshot.id, Uuid::parse_str(SNAPSHOT).unwrap());
    assert_eq!(snapshot.collection_watermark, 7);
    let requests = server.requests();
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0].method, "POST");
    assert_eq!(requests[0].path, "/api/v1/catalog/snapshots");
    assert_eq!(requests[0].body, json!({"library_id": "owned-library"}));
    assert_eq!(
        requests[0].headers.get("authorization"),
        Some(&format!("Bearer {TOKEN}"))
    );
    assert_eq!(
        requests[0]
            .headers
            .get("x-hito-account")
            .map(String::as_str),
        Some("owned-account")
    );
    assert_eq!(
        requests[0]
            .headers
            .get("x-hito-request")
            .map(String::as_str),
        Some("1")
    );
    assert!(!requests[0].headers.contains_key("cookie"));
    server.close().await;
}

#[tokio::test]
async fn reads_a_typed_page_path_and_counts_actual_wire_bytes() {
    let page = json!({"snapshot_id": SNAPSHOT, "ordinal": u64::MAX, "recordings": [], "next_ordinal": null});
    let mut reply = super::fixture::Reply::json(page.clone());
    let bytes = serde_json::to_vec_pretty(&page).unwrap();
    let expected_bytes = bytes.len() + 17;
    reply.chunks = vec![
        (std::time::Duration::ZERO, bytes),
        (std::time::Duration::ZERO, vec![b' '; 17]),
    ];
    let server = Server::scripted(vec![reply]).await;
    let remote = Remote::new(Endpoint::parse(&server.endpoint).unwrap()).unwrap();
    let (received, size) = remote
        .catalog_snapshot_page(
            TOKEN,
            "owned-account",
            Uuid::parse_str(SNAPSHOT).unwrap(),
            u64::MAX,
        )
        .await
        .unwrap();
    assert_eq!(received.ordinal, u64::MAX);
    assert_eq!(received.snapshot_id, Uuid::parse_str(SNAPSHOT).unwrap());
    assert_eq!(size, expected_bytes);
    let requests = server.requests();
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0].method, "GET");
    assert_eq!(
        requests[0].path,
        format!("/api/v1/catalog/snapshots/{SNAPSHOT}/pages/{}", u64::MAX)
    );
    assert_eq!(
        requests[0].headers.get("authorization"),
        Some(&format!("Bearer {TOKEN}"))
    );
    assert_eq!(
        requests[0]
            .headers
            .get("x-hito-account")
            .map(String::as_str),
        Some("owned-account")
    );
    assert!(requests[0].body.is_null());
    server.close().await;
}

#[tokio::test]
async fn releases_a_lease_with_delete_and_accepts_the_servers_empty_204() {
    let mut reply = super::fixture::Reply::status(204);
    reply.headers.clear();
    reply.chunks.clear();
    let server = Server::scripted(vec![reply]).await;
    let remote = Remote::new(Endpoint::parse(&server.endpoint).unwrap()).unwrap();
    remote
        .release_catalog_snapshot(TOKEN, "owned-account", Uuid::parse_str(SNAPSHOT).unwrap())
        .await
        .unwrap();
    let requests = server.requests();
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0].method, "DELETE");
    assert_eq!(
        requests[0].path,
        format!("/api/v1/catalog/snapshots/{SNAPSHOT}")
    );
    assert_eq!(
        requests[0].headers.get("authorization"),
        Some(&format!("Bearer {TOKEN}"))
    );
    assert_eq!(
        requests[0]
            .headers
            .get("x-hito-account")
            .map(String::as_str),
        Some("owned-account")
    );
    assert_eq!(
        requests[0]
            .headers
            .get("x-hito-request")
            .map(String::as_str),
        Some("1")
    );
    assert!(requests[0].body.is_null());
    server.close().await;
}

#[tokio::test]
async fn catalog_errors_are_classified_without_consuming_bodies_or_retrying() {
    use super::transport::RemoteError;
    use std::time::Duration;
    for (status, expected) in [
        (401, RemoteError::Unauthorized),
        (403, RemoteError::Forbidden),
        (404, RemoteError::NotFound),
        (409, RemoteError::Conflict),
        (423, RemoteError::Locked),
        (
            429,
            RemoteError::RateLimited {
                retry_after: Some(Duration::from_secs(31)),
            },
        ),
        (503, RemoteError::Unavailable),
        (307, RemoteError::InvalidReply),
    ] {
        let mut replies = Vec::new();
        for _ in 0..3 {
            let mut reply = super::fixture::Reply::status(status);
            reply.headers.insert("retry-after", "31".parse().unwrap());
            reply.chunks = vec![(Duration::from_secs(3), b"private-error-body".to_vec())];
            replies.push(reply);
        }
        let server = Server::scripted(replies).await;
        let remote = Remote::with_deadlines(
            Endpoint::parse(&server.endpoint).unwrap(),
            Duration::from_secs(1),
            Duration::from_millis(100),
        )
        .unwrap();
        assert!(
            matches!(remote.begin_catalog_snapshot(TOKEN, "owned-account", &BeginCatalogSnapshot { library_id: "owned-library".into() }).await, Err(error) if error == expected)
        );
        assert!(
            matches!(remote.catalog_snapshot_page(TOKEN, "owned-account", Uuid::parse_str(SNAPSHOT).unwrap(), 0).await, Err(error) if error == expected)
        );
        assert_eq!(
            remote
                .release_catalog_snapshot(
                    TOKEN,
                    "owned-account",
                    Uuid::parse_str(SNAPSHOT).unwrap()
                )
                .await
                .unwrap_err(),
            expected
        );
        assert_eq!(
            server.requests().len(),
            3,
            "catalog request retried implicitly"
        );
        server.close().await;
    }
}

#[derive(Clone, Copy, Debug)]
pub(super) enum Call {
    Begin,
    Page,
    Release,
}

impl Call {
    pub(super) const ALL: [Self; 3] = [Self::Begin, Self::Page, Self::Release];

    pub(super) async fn run(self, remote: &Remote) -> super::transport::RemoteResult<()> {
        match self {
            Self::Begin => remote
                .begin_catalog_snapshot(
                    TOKEN,
                    "owned-account",
                    &BeginCatalogSnapshot {
                        library_id: "private-library".into(),
                    },
                )
                .await
                .map(|_| ()),
            Self::Page => remote
                .catalog_snapshot_page(
                    TOKEN,
                    "owned-account",
                    Uuid::parse_str(SNAPSHOT).unwrap(),
                    0,
                )
                .await
                .map(|_| ()),
            Self::Release => {
                remote
                    .release_catalog_snapshot(
                        TOKEN,
                        "owned-account",
                        Uuid::parse_str(SNAPSHOT).unwrap(),
                    )
                    .await
            }
        }
    }
}
