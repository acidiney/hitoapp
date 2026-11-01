use super::{ANOTHER_ARTIST_ID, ARTIST_ID};
use crate::MusicBrainzClient;
use axum::{
    Json, Router,
    body::to_bytes,
    extract::{Request, State},
    http::{HeaderMap, Method, StatusCode, Uri},
    response::{IntoResponse, Response},
};
use futures_util::FutureExt;
use hito_plugin_sdk::{ArtistSearchPage, ArtistSearchRequest, CatalogArtistKind};
use serde_json::json;
use std::{
    panic::{AssertUnwindSafe, resume_unwind},
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio::{net::TcpListener, sync::oneshot, task::JoinHandle};

const QUERY: &str = r#"Björk / A+B - C && D || E ! (F) {G} [H] ^ "I" ~ J* K? L: M\N"#;
const LITERAL: &str =
    r#""Björk \/ A\+B \- C \&\& D \|\| E \! \(F\) \{G\} \[H\] \^ \"I\" \~ J\* K\? L\: M\\N""#;
const THIRD_ARTIST_ID: &str = "0f700000-0000-4000-8000-000000000003";

struct ObservedRequest {
    method: Method,
    uri: Uri,
    headers: HeaderMap,
    body: Vec<u8>,
}

struct Fixture {
    endpoint: reqwest::Url,
    requests: Arc<Mutex<Vec<ObservedRequest>>>,
    stop: Option<oneshot::Sender<()>>,
    server: Option<JoinHandle<bool>>,
}

impl Fixture {
    async fn start() -> Self {
        let requests = Arc::new(Mutex::new(Vec::new()));
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let endpoint = format!("http://{}/ws/2/", listener.local_addr().unwrap())
            .parse()
            .unwrap();
        let router = Router::new().fallback(respond).with_state(requests.clone());
        let (stop, stopped) = oneshot::channel();
        let server = tokio::spawn(async move {
            axum::serve(listener, router)
                .with_graceful_shutdown(async move {
                    let _ = stopped.await;
                })
                .await
                .is_ok()
        });
        Self {
            endpoint,
            requests,
            stop: Some(stop),
            server: Some(server),
        }
    }

    async fn close(&mut self) -> bool {
        if let Some(stop) = self.stop.take() {
            let _ = stop.send(());
        }
        let Some(mut server) = self.server.take() else {
            return true;
        };
        match tokio::time::timeout(Duration::from_secs(2), &mut server).await {
            Ok(result) => matches!(result, Ok(true)),
            Err(_) => {
                server.abort();
                let _ = server.await;
                false
            }
        }
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        if let Some(stop) = self.stop.take() {
            let _ = stop.send(());
        }
        if let Some(server) = self.server.take() {
            server.abort();
        }
    }
}

async fn respond(
    State(requests): State<Arc<Mutex<Vec<ObservedRequest>>>>,
    request: Request,
) -> Response {
    let (parts, body) = request.into_parts();
    let Ok(body) = to_bytes(body, 1024).await else {
        return StatusCode::BAD_REQUEST.into_response();
    };
    let mut requests = requests.lock().unwrap();
    if requests.len() >= 4 {
        return StatusCode::TOO_MANY_REQUESTS.into_response();
    }
    let index = requests.len();
    requests.push(ObservedRequest {
        method: parts.method,
        uri: parts.uri,
        headers: parts.headers,
        body: body.to_vec(),
    });
    let (offset, artists) = if index == 0 {
        (
            0,
            json!([
                {"id": ARTIST_ID, "name": "Owned Ensemble", "type": "Group", "disambiguation": "Owned acoustic ensemble", "score": "100"},
                {"id": ANOTHER_ARTIST_ID, "name": "Owned Ensemble", "type": null, "disambiguation": "", "score": "100"}
            ]),
        )
    } else {
        (
            2,
            json!([
                {"id": THIRD_ARTIST_ID, "name": "Owned other performer", "type": null, "disambiguation": null, "score": "79"}
            ]),
        )
    };
    Json(json!({
        "created": "2026-10-18T12:34:56.000Z", "count": 3, "offset": offset, "artists": artists
    }))
    .into_response()
}

fn request(cursor: Option<String>) -> ArtistSearchRequest {
    ArtistSearchRequest {
        query: QUERY.into(),
        cursor,
        snapshot: None,
        limit: 2,
    }
}

#[tokio::test]
async fn artist_search_escapes_literal_queries_and_preserves_distinct_paginated_candidates() {
    let mut fixture = Fixture::start().await;
    let result = AssertUnwindSafe(exercise(&fixture)).catch_unwind().await;
    let joined = fixture.close().await;
    let (first, last) = match result {
        Ok(pages) => pages,
        Err(panic) => resume_unwind(panic),
    };
    assert!(joined, "The owned artist-search fixture must join shutdown");
    assert_eq!(first.query, QUERY);
    assert_eq!(first.total_count, Some(3));
    assert!(
        first.snapshot.is_none(),
        "The response creation timestamp is not a stable search snapshot"
    );
    assert_eq!(first.items.len(), 2);
    assert_eq!(first.items[0].artist.id, ARTIST_ID);
    assert_eq!(first.items[1].artist.id, ANOTHER_ARTIST_ID);
    assert_eq!(first.items[0].artist.name, first.items[1].artist.name);
    assert!(first.items[0].kind == Some(CatalogArtistKind::Group));
    assert_eq!(
        first.items[0].disambiguation.as_deref(),
        Some("Owned acoustic ensemble")
    );
    assert!(first.items[1].kind.is_none());
    assert!(first.items[1].disambiguation.is_none());
    assert_eq!(last.query, QUERY);
    assert_eq!(last.total_count, Some(3));
    assert!(last.snapshot.is_none());
    assert_eq!(last.items.len(), 1);
    assert_eq!(last.items[0].artist.id, THIRD_ARTIST_ID);
    assert!(last.next_cursor.is_none());

    let requests = fixture.requests.lock().unwrap();
    assert_eq!(
        requests.len(),
        2,
        "Explicit search continuation must not crawl or retry"
    );
    for (observed, offset) in requests.iter().zip([0, 2]) {
        let mut expected = fixture.endpoint.join("artist").unwrap();
        expected.query_pairs_mut().extend_pairs([
            ("query", LITERAL),
            ("fmt", "json"),
            ("limit", "2"),
            ("offset", &offset.to_string()),
        ]);
        assert_eq!(observed.method, Method::GET);
        assert_eq!(
            observed.uri.to_string(),
            format!("{}?{}", expected.path(), expected.query().unwrap())
        );
        assert!(observed.body.is_empty());
        assert_eq!(
            observed
                .headers
                .get("accept")
                .and_then(|value| value.to_str().ok()),
            Some("application/json")
        );
        assert_eq!(
            observed
                .headers
                .get("accept-encoding")
                .and_then(|value| value.to_str().ok()),
            Some("identity")
        );
        assert_eq!(
            observed
                .headers
                .get("user-agent")
                .and_then(|value| value.to_str().ok()),
            Some(concat!("Hito/", env!("CARGO_PKG_VERSION")))
        );
        for private_header in ["authorization", "cookie", "x-hito-plugin-protocol"] {
            assert!(observed.headers.get(private_header).is_none());
        }
    }
}

async fn exercise(fixture: &Fixture) -> (ArtistSearchPage, ArtistSearchPage) {
    let client = MusicBrainzClient::with_test_endpoint(fixture.endpoint.clone()).unwrap();
    assert!(
        fixture.requests.lock().unwrap().is_empty(),
        "Constructing the client must not contact its provider"
    );
    let first = client
        .artist_search(request(None))
        .await
        .expect("The first owned search must succeed");
    first
        .validate_for(&request(None))
        .expect("The first page must satisfy the shared discovery contract");
    let cursor = first
        .next_cursor
        .clone()
        .expect("The provider's remaining candidates require an explicit continuation");
    assert!(cursor.len() <= 2048);
    let continuation = request(Some(cursor));
    let last = client
        .artist_search(request(continuation.cursor.clone()))
        .await
        .expect("The owned continuation must succeed");
    last.validate_for(&continuation)
        .expect("The continuation must preserve its search context");
    (first, last)
}
