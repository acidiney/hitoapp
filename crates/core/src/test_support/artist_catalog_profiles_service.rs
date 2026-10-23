use axum::{
    Json, Router,
    extract::State,
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::{get, post},
};
use hito_plugin_sdk::{
    ArtistProfileRequest, CATALOG_VERSION, Capability, CatalogArtistKind, CatalogArtistProfile,
    CatalogArtistRef, CatalogCapabilities, CatalogOperation, Manifest, PROTOCOL_VERSION,
};
use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    time::Duration,
};
use tokio::{
    sync::{Mutex, Semaphore, oneshot},
    task::JoinHandle,
};

pub const ARTIST_ID: &str = "opaque:artist/one?credit=lead";
pub const MISSING_DESCRIPTOR_BODY: &str = "private-catalog-descriptor-404-body-canary";

#[derive(Debug, PartialEq, Eq)]
pub struct Counts {
    pub manifests: usize,
    pub descriptors: usize,
    pub profiles: usize,
    pub unexpected: usize,
}

#[derive(Default)]
struct Requests {
    manifests: AtomicUsize,
    descriptors: AtomicUsize,
    descriptor_missing: AtomicBool,
    profiles: Mutex<Vec<String>>,
    unexpected: AtomicUsize,
    profile_gate: Mutex<Option<Arc<ProfileGate>>>,
}

pub struct ProfileGate {
    entered: Semaphore,
    release: Semaphore,
    retired: Semaphore,
}

impl ProfileGate {
    pub async fn entered(&self) {
        self.entered.acquire().await.unwrap().forget();
    }

    pub async fn retired(&self) {
        self.retired.acquire().await.unwrap().forget();
    }
}

struct HeldProfile(Arc<ProfileGate>);

impl Drop for HeldProfile {
    fn drop(&mut self) {
        self.0.retired.add_permits(1);
    }
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
        let router = Router::new()
            .route("/prefix/v1/manifest", get(manifest))
            .route("/prefix/v1/catalog/capabilities", get(capabilities))
            .route("/prefix/v1/catalog/artists/profile", post(profile))
            .fallback(unexpected)
            .with_state(requests.clone());
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

    pub async fn counts(&self) -> Counts {
        Counts {
            manifests: self.requests.manifests.load(Ordering::SeqCst),
            descriptors: self.requests.descriptors.load(Ordering::SeqCst),
            profiles: self.requests.profiles.lock().await.len(),
            unexpected: self.requests.unexpected.load(Ordering::SeqCst),
        }
    }

    pub async fn profile_requests(&self) -> Vec<String> {
        self.requests.profiles.lock().await.clone()
    }

    pub fn set_descriptor_missing(&self, missing: bool) {
        self.requests
            .descriptor_missing
            .store(missing, Ordering::SeqCst);
    }

    pub async fn hold_profile(&self) -> Arc<ProfileGate> {
        let gate = Arc::new(ProfileGate {
            entered: Semaphore::new(0),
            release: Semaphore::new(0),
            retired: Semaphore::new(0),
        });
        *self.requests.profile_gate.lock().await = Some(gate.clone());
        gate
    }

    pub async fn release_profile(&self) {
        if let Some(gate) = self.requests.profile_gate.lock().await.take() {
            gate.release.add_permits(1);
        }
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

async fn manifest(State(requests): State<Arc<Requests>>) -> Json<Manifest> {
    requests.manifests.fetch_add(1, Ordering::SeqCst);
    Json(Manifest {
        id: "org.hito.artist-catalog-fixture".into(),
        name: "Owned artist catalog".into(),
        protocol_version: PROTOCOL_VERSION,
        capabilities: [Capability::Catalog].into(),
        authorization: None,
        content_policy: None,
    })
}

async fn capabilities(State(requests): State<Arc<Requests>>) -> Response {
    requests.descriptors.fetch_add(1, Ordering::SeqCst);
    if requests.descriptor_missing.load(Ordering::SeqCst) {
        return (StatusCode::NOT_FOUND, MISSING_DESCRIPTOR_BODY).into_response();
    }
    Json(CatalogCapabilities {
        catalog_version: CATALOG_VERSION,
        operations: vec![CatalogOperation::ArtistProfile],
    })
    .into_response()
}

async fn profile(
    State(requests): State<Arc<Requests>>,
    Json(request): Json<ArtistProfileRequest>,
) -> Response {
    let mut profiles = requests.profiles.lock().await;
    if request.validate().is_err() || request.artist_id != ARTIST_ID || profiles.len() >= 16 {
        requests.unexpected.fetch_add(1, Ordering::SeqCst);
        return StatusCode::BAD_REQUEST.into_response();
    }
    profiles.push(request.artist_id);
    drop(profiles);
    let gate = requests.profile_gate.lock().await.clone();
    if let Some(gate) = gate {
        let _held = HeldProfile(gate.clone());
        gate.entered.add_permits(1);
        gate.release.acquire().await.unwrap().forget();
    }
    Json(expected_profile()).into_response()
}

async fn unexpected(State(requests): State<Arc<Requests>>) -> StatusCode {
    requests.unexpected.fetch_add(1, Ordering::SeqCst);
    StatusCode::NOT_FOUND
}

pub fn expected_profile() -> CatalogArtistProfile {
    CatalogArtistProfile {
        artist: CatalogArtistRef {
            id: ARTIST_ID.into(),
            name: "Verified provider artist".into(),
        },
        sort_name: Some("Provider artist, Verified".into()),
        kind: Some(CatalogArtistKind::Group),
        disambiguation: None,
        revision: Some("owned-profile-revision-1".into()),
    }
}
