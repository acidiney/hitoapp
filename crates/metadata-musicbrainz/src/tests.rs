use crate::MusicBrainzClient;
use axum::{
    Json, Router,
    extract::{Request, State},
    http::{HeaderMap, Method, Uri},
};
use hito_plugin_sdk::ArtistProfileRequest;
use serde_json::{Value, json};
use std::sync::Arc;
use tokio::{
    net::TcpListener,
    sync::{Mutex, oneshot},
    task::JoinHandle,
};

mod cover_art;
mod discography;
mod discography_context;
mod discography_transport;
mod discography_type;
mod edition;
mod edition_listing;
mod edition_validation;
mod http;
mod isrc;
mod limiter;
mod protocol;
mod recordings;
mod search;
mod search_context;
mod search_limiter;
mod search_protocol;

const ARTIST_ID: &str = "0f700000-0000-4000-8000-000000000001";
const ANOTHER_ARTIST_ID: &str = "0f700000-0000-4000-8000-000000000002";

struct ObservedRequest {
    method: Method,
    uri: Uri,
    headers: HeaderMap,
}

struct Fixture {
    endpoint: reqwest::Url,
    requests: Arc<Mutex<Vec<ObservedRequest>>>,
    stop: Option<oneshot::Sender<()>>,
    server: Option<JoinHandle<()>>,
}

impl Fixture {
    async fn start() -> Self {
        let requests = Arc::new(Mutex::new(Vec::new()));
        let router = Router::new().fallback(profile).with_state(requests.clone());
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("the owned metadata fixture must bind");
        let endpoint = format!(
            "http://{}/ws/2/",
            listener
                .local_addr()
                .expect("the owned metadata fixture must have an address")
        )
        .parse()
        .expect("the owned metadata endpoint must be valid");
        let (stop, stopped) = oneshot::channel();
        let server = tokio::spawn(async move {
            axum::serve(listener, router)
                .with_graceful_shutdown(async move {
                    let _ = stopped.await;
                })
                .await
                .expect("the owned metadata fixture server must run");
        });
        Self {
            endpoint,
            requests,
            stop: Some(stop),
            server: Some(server),
        }
    }

    async fn close(mut self) {
        if let Some(stop) = self.stop.take() {
            let _ = stop.send(());
        }
        if let Some(server) = self.server.take() {
            server.await.expect("the owned metadata fixture must stop");
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

async fn profile(
    State(requests): State<Arc<Mutex<Vec<ObservedRequest>>>>,
    request: Request,
) -> Json<Value> {
    let mut requests = requests.lock().await;
    let id = if requests.is_empty() {
        ARTIST_ID
    } else {
        ANOTHER_ARTIST_ID
    };
    requests.push(ObservedRequest {
        method: request.method().clone(),
        uri: request.uri().clone(),
        headers: request.headers().clone(),
    });
    Json(json!({
        "id": id,
        "name": "Owned fixture ensemble",
        "sort-name": null,
        "type": null,
        "disambiguation": ""
    }))
}

#[tokio::test]
async fn artist_profile_uses_identified_json_requests_and_rejects_another_returned_artist() {
    let fixture = Fixture::start().await;
    let client = MusicBrainzClient::with_test_endpoint(fixture.endpoint.clone())
        .expect("the owned MusicBrainz client must initialize");
    assert!(fixture.requests.lock().await.is_empty());
    let profile = client
        .artist_profile(ArtistProfileRequest {
            artist_id: ARTIST_ID.into(),
        })
        .await
        .expect("the requested owned artist profile must load");
    assert_eq!(
        serde_json::to_value(profile).expect("the owned artist profile must serialize"),
        json!({
            "artist": {"id": ARTIST_ID, "name": "Owned fixture ensemble"},
            "sort_name": null,
            "kind": null,
            "disambiguation": null,
            "revision": null
        })
    );
    assert!(
        client
            .artist_profile(ArtistProfileRequest {
                artist_id: ARTIST_ID.into()
            })
            .await
            .is_err(),
        "a different returned MusicBrainz artist must be rejected"
    );

    let requests = fixture.requests.lock().await;
    assert_eq!(requests.len(), 2);
    for request in requests.iter() {
        assert_eq!(request.method, Method::GET);
        assert_eq!(
            request.uri.to_string(),
            format!("/ws/2/artist/{ARTIST_ID}?fmt=json")
        );
        assert_eq!(
            request
                .headers
                .get("accept")
                .and_then(|value| value.to_str().ok()),
            Some("application/json")
        );
        assert_eq!(
            request
                .headers
                .get("accept-encoding")
                .and_then(|value| value.to_str().ok()),
            Some("identity")
        );
        assert_eq!(
            request
                .headers
                .get("user-agent")
                .and_then(|value| value.to_str().ok()),
            Some(concat!(
                "Hito/",
                env!("CARGO_PKG_VERSION"),
                " (contact@example.invalid)"
            ))
        );
        assert!(request.headers.get("authorization").is_none());
        assert!(request.headers.get("cookie").is_none());
    }
    drop(requests);
    drop(client);
    fixture.close().await;
}
