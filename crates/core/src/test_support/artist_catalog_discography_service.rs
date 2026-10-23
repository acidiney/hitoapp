use axum::{
    Json, Router,
    body::to_bytes,
    extract::{Request, State},
    http::{Method, StatusCode},
    response::{IntoResponse, Response},
};
use hito_plugin_sdk::{
    ArtistDiscographyPage, ArtistDiscographyRequest, ArtistProfileRequest, CATALOG_VERSION,
    Capability, CatalogArtistKind, CatalogArtistProfile, CatalogArtistRef, CatalogCapabilities,
    CatalogCreditMember, CatalogOperation, CatalogReleaseGroup, CatalogReleaseGroupKind, Manifest,
    PROTOCOL_VERSION,
};
use serde_json::Value;
use std::{
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    time::Duration,
};
use tokio::{sync::oneshot, task::JoinHandle};

pub const ARTIST_ID: &str = "opaque:artista/á?credit=lead&edição=一";
pub const PROVIDER_CURSOR: &str = "opaque:discografia/次?cursor=2%2F&assinatura=privada";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ObservedRequest {
    pub method: Method,
    pub path: String,
    pub body: Value,
}

#[derive(Default)]
struct Requests {
    discography_enabled: AtomicBool,
    observed: Mutex<Vec<ObservedRequest>>,
    unexpected: AtomicUsize,
}

pub struct CatalogServer {
    pub url: String,
    requests: Arc<Requests>,
    stop: Option<oneshot::Sender<()>>,
    task: Option<JoinHandle<bool>>,
}

impl CatalogServer {
    pub async fn start() -> Self {
        let requests = Arc::new(Requests::default());
        let router = Router::new().fallback(respond).with_state(requests.clone());
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}/prefix/", listener.local_addr().unwrap());
        let (stop, stopped) = oneshot::channel();
        let task = tokio::spawn(async move {
            axum::serve(listener, router)
                .with_graceful_shutdown(async {
                    let _ = stopped.await;
                })
                .await
                .is_ok()
        });
        Self {
            url,
            requests,
            stop: Some(stop),
            task: Some(task),
        }
    }

    pub fn enable_discography(&self) {
        self.requests
            .discography_enabled
            .store(true, Ordering::SeqCst);
    }

    pub fn requests(&self) -> Vec<ObservedRequest> {
        self.requests.observed.lock().unwrap().clone()
    }

    pub fn unexpected(&self) -> usize {
        self.requests.unexpected.load(Ordering::SeqCst)
    }

    pub async fn finish(&mut self) -> bool {
        if let Some(stop) = self.stop.take() {
            let _ = stop.send(());
        }
        let Some(mut task) = self.task.take() else {
            return true;
        };
        match tokio::time::timeout(Duration::from_secs(3), &mut task).await {
            Ok(result) => matches!(result, Ok(true)),
            Err(_) => {
                task.abort();
                let _ = task.await;
                false
            }
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

async fn respond(State(requests): State<Arc<Requests>>, request: Request) -> Response {
    let (parts, body) = request.into_parts();
    let Ok(bytes) = to_bytes(body, 4096).await else {
        return reject(&requests);
    };
    let body = if bytes.is_empty() {
        Value::Null
    } else {
        let Ok(value) = serde_json::from_slice(&bytes) else {
            return reject(&requests);
        };
        value
    };
    if parts.uri.query().is_some() {
        return reject(&requests);
    }
    let path = parts.uri.path().to_owned();
    let mut observed = requests.observed.lock().unwrap();
    if observed.len() >= 16 {
        return reject(&requests);
    }
    observed.push(ObservedRequest {
        method: parts.method.clone(),
        path: path.clone(),
        body: body.clone(),
    });
    drop(observed);
    match (parts.method.as_str(), path.as_str()) {
        ("GET", "/prefix/v1/manifest") if bytes.is_empty() => Json(Manifest {
            id: "org.hito.artist-discography-fixture".into(),
            name: "Owned artist discography".into(),
            protocol_version: PROTOCOL_VERSION,
            capabilities: [Capability::Catalog].into(),
            authorization: None,
            content_policy: None,
        })
        .into_response(),
        ("GET", "/prefix/v1/catalog/capabilities") if bytes.is_empty() => {
            let mut operations = vec![CatalogOperation::ArtistProfile];
            if requests.discography_enabled.load(Ordering::SeqCst) {
                operations.push(CatalogOperation::ArtistDiscography);
            }
            Json(CatalogCapabilities {
                catalog_version: CATALOG_VERSION,
                operations,
            })
            .into_response()
        }
        ("POST", "/prefix/v1/catalog/artists/profile") => {
            let Ok(request) = serde_json::from_value::<ArtistProfileRequest>(body) else {
                return reject(&requests);
            };
            if request.validate().is_err() || request.artist_id != ARTIST_ID {
                return reject(&requests);
            }
            Json(expected_profile()).into_response()
        }
        ("POST", "/prefix/v1/catalog/artists/discography") => {
            let Ok(request) = serde_json::from_value::<ArtistDiscographyRequest>(body) else {
                return reject(&requests);
            };
            if !requests.discography_enabled.load(Ordering::SeqCst)
                || request.validate().is_err()
                || request.artist_id != ARTIST_ID
                || request.snapshot.is_some()
                || request.limit != 1
                || !matches!(request.cursor.as_deref(), None | Some(PROVIDER_CURSOR))
            {
                return reject(&requests);
            }
            Json(expected_page(request.cursor.is_some())).into_response()
        }
        _ => reject(&requests),
    }
}

fn reject(requests: &Requests) -> Response {
    requests.unexpected.fetch_add(1, Ordering::SeqCst);
    StatusCode::BAD_REQUEST.into_response()
}

pub fn expected_profile() -> CatalogArtistProfile {
    CatalogArtistProfile {
        artist: CatalogArtistRef {
            id: ARTIST_ID.into(),
            name: "Conjunto da sessão".into(),
        },
        sort_name: None,
        kind: Some(CatalogArtistKind::Group),
        disambiguation: None,
        revision: None,
    }
}

pub fn expected_page(last: bool) -> ArtistDiscographyPage {
    let group = if last {
        CatalogReleaseGroup {
            id: "opaque:release-group/estúdio?edição=二".into(),
            title: "Sessão nº 1".into(),
            artist_credit: None,
            kind: None,
            secondary_types: vec![],
            first_release_date: None,
            revision: None,
        }
    } else {
        CatalogReleaseGroup {
            id: "opaque:release-group/ao-vivo?edição=一".into(),
            title: "Sessão nº 1".into(),
            artist_credit: Some(vec![
                CatalogCreditMember {
                    artist: Some(expected_profile().artist),
                    credited_name: "O conjunto da sessão".into(),
                    join_phrase: " & ".into(),
                },
                CatalogCreditMember {
                    artist: None,
                    credited_name: "Voz convidada".into(),
                    join_phrase: String::new(),
                },
            ]),
            kind: Some(CatalogReleaseGroupKind::Album),
            secondary_types: vec!["Live".into()],
            first_release_date: Some("2024-02".into()),
            revision: None,
        }
    };
    ArtistDiscographyPage {
        artist_id: ARTIST_ID.into(),
        snapshot: None,
        items: vec![group],
        total_count: None,
        next_cursor: (!last).then(|| PROVIDER_CURSOR.into()),
    }
}
