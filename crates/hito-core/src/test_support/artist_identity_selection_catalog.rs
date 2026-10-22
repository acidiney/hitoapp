use axum::{
    Json, Router,
    body::to_bytes,
    extract::{Request, State},
    http::{Method, StatusCode},
    response::{IntoResponse, Response},
};
use hito_plugin_sdk::{
    ArtistProfileRequest, ArtistSearchRequest, CATALOG_DISCOVERY_VERSION, CATALOG_VERSION,
    CatalogArtistProfile, CatalogArtistRef, CatalogCapabilities, CatalogDiscoveryCapabilities,
    CatalogDiscoveryOperation, CatalogOperation,
};
use serde_json::{Value, json};
use std::sync::{Arc, Mutex};
use tokio::{sync::oneshot, task::JoinHandle};

pub const CREDIT: &str = "C4 Pedro";
pub const ALPHA: &str = "owned:artist/alpha";
pub const BETA: &str = "owned:artist/beta";

type Requests = Arc<Mutex<Vec<(Method, String, Value)>>>;

pub struct CatalogServer {
    pub url: String,
    requests: Requests,
    stop: Option<oneshot::Sender<()>>,
    task: Option<JoinHandle<()>>,
}

impl CatalogServer {
    pub async fn start() -> Self {
        let requests = Arc::new(Mutex::new(Vec::new()));
        let router = Router::new().fallback(respond).with_state(requests.clone());
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}/", listener.local_addr().unwrap());
        let (stop, stopped) = oneshot::channel();
        let task = tokio::spawn(async move {
            axum::serve(listener, router)
                .with_graceful_shutdown(async {
                    let _ = stopped.await;
                })
                .await
                .unwrap();
        });
        Self {
            url,
            requests,
            stop: Some(stop),
            task: Some(task),
        }
    }

    pub fn requests(&self) -> Vec<(Method, String, Value)> {
        self.requests.lock().unwrap().clone()
    }

    pub async fn finish(&mut self) {
        if let Some(stop) = self.stop.take() {
            let _ = stop.send(());
        }
        if let Some(task) = self.task.take() {
            tokio::time::timeout(std::time::Duration::from_secs(3), task)
                .await
                .unwrap()
                .unwrap();
        }
    }
}

impl Drop for CatalogServer {
    fn drop(&mut self) {
        if let Some(stop) = self.stop.take() {
            let _ = stop.send(());
        }
        if let Some(task) = self.task.take() {
            task.abort();
        }
    }
}

async fn respond(State(requests): State<Requests>, request: Request) -> Response {
    let (parts, body) = request.into_parts();
    let body = to_bytes(body, 4096).await.unwrap();
    let value = if body.is_empty() {
        Value::Null
    } else {
        serde_json::from_slice(&body).unwrap()
    };
    let path = parts.uri.path();
    requests
        .lock()
        .unwrap()
        .push((parts.method.clone(), path.into(), value.clone()));
    match (parts.method.as_str(), path) {
        ("GET", "/v1/manifest") => Json(json!({
            "id": "org.hito.artist-identity-fixture", "name": "Owned artist identities",
            "protocol_version": 1, "capabilities": ["catalog"]
        }))
        .into_response(),
        ("GET", "/v1/catalog/discovery/capabilities") => Json(CatalogDiscoveryCapabilities {
            discovery_version: CATALOG_DISCOVERY_VERSION,
            operations: vec![CatalogDiscoveryOperation::ArtistSearch],
        })
        .into_response(),
        ("GET", "/v1/catalog/capabilities") => Json(CatalogCapabilities {
            catalog_version: CATALOG_VERSION,
            operations: vec![CatalogOperation::ArtistProfile],
        })
        .into_response(),
        ("POST", "/v1/catalog/artists/search") => {
            let request: ArtistSearchRequest = serde_json::from_value(value).unwrap();
            assert!(request.validate().is_ok() && request.query == CREDIT);
            Json(json!({
                "query": request.query, "snapshot": "owned-identities-v1",
                "items": [
                    {"artist": {"id": ALPHA, "name": CREDIT}, "kind": null, "disambiguation": "Owned first performer"},
                    {"artist": {"id": BETA, "name": CREDIT}, "kind": null, "disambiguation": "Owned namesake"}
                ], "total_count": 2, "next_cursor": null
            })).into_response()
        }
        ("POST", "/v1/catalog/artists/profile") => {
            let request: ArtistProfileRequest = serde_json::from_value(value).unwrap();
            assert!(
                request.validate().is_ok() && [ALPHA, BETA].contains(&request.artist_id.as_str())
            );
            Json(CatalogArtistProfile {
                artist: CatalogArtistRef {
                    id: request.artist_id,
                    name: CREDIT.into(),
                },
                sort_name: None,
                kind: None,
                disambiguation: None,
                revision: Some("owned-profile-v1".into()),
            })
            .into_response()
        }
        _ => StatusCode::BAD_REQUEST.into_response(),
    }
}
