use super::{
    API_HOST, FILE, HASH, HOSTER_LINK, MEDIA_HOST, MEDIA_TARGET, TORRENT, audio, http,
    media::MediaGate,
};
use axum::{
    Extension, Json, Router,
    extract::{Request, State},
    http::{HeaderMap, HeaderValue, StatusCode},
    response::{IntoResponse, Response},
};
use hito_testkit::tls::OwnedAuthority;
use hyper_util::{rt::TokioIo, service::TowerToHyperService};
use serde_json::json;
use std::{
    net::SocketAddr,
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio::{
    net::TcpListener,
    sync::oneshot,
    task::{JoinHandle, JoinSet},
};

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Origin {
    Api,
    Media,
}

pub struct Exchange {
    pub origin: Origin,
    pub server_name: String,
    pub method: String,
    pub target: String,
    pub headers: HeaderMap,
    pub body: Vec<u8>,
}

#[derive(Debug, PartialEq, Eq)]
pub struct Counts {
    pub observations: usize,
    pub posts: usize,
    pub media: usize,
}

#[derive(Clone)]
struct Handler {
    origin: Origin,
    exchanges: Arc<Mutex<Vec<Exchange>>>,
    audio: Arc<Vec<u8>>,
    media_gate: Option<Arc<MediaGate>>,
    content_type: &'static str,
    file_path: Arc<Mutex<String>>,
}

pub struct Provider {
    ca: Vec<u8>,
    pub api_address: SocketAddr,
    pub media_address: SocketAddr,
    exchanges: Arc<Mutex<Vec<Exchange>>>,
    file_path: Arc<Mutex<String>>,
    stop: Option<oneshot::Sender<()>>,
    task: Option<JoinHandle<Result<(), &'static str>>>,
}

impl Provider {
    pub async fn start(wav: Vec<u8>) -> Self {
        assert_eq!(wav.len(), audio::WAV_BYTES);
        Self::start_audio(wav, "audio/wav", FILE, None).await
    }

    pub async fn start_audio(
        audio: Vec<u8>,
        content_type: &'static str,
        path: &str,
        media_gate: Option<Arc<MediaGate>>,
    ) -> Self {
        assert!(!audio.is_empty() && audio.len() <= 1_048_576);
        assert!(["audio/wav", "audio/mpeg"].contains(&content_type));
        assert!(path.starts_with('/') && path.len() <= 4096);
        let authority = OwnedAuthority::new();
        let ca_der = authority.root_der.clone();
        let api_tls = authority.acceptor(API_HOST);
        let media_tls = authority.acceptor(MEDIA_HOST);
        let api = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("owned API listener");
        let media = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("owned media listener");
        let api_address = api.local_addr().expect("owned API address");
        let media_address = media.local_addr().expect("owned media address");
        let exchanges = Arc::new(Mutex::new(Vec::new()));
        let file_path = Arc::new(Mutex::new(path.to_owned()));
        let state = Handler {
            origin: Origin::Api,
            exchanges: exchanges.clone(),
            audio: Arc::new(audio),
            media_gate,
            content_type,
            file_path: file_path.clone(),
        };
        let (stop, mut stopped) = oneshot::channel();
        let task = tokio::spawn(async move {
            let mut handlers = JoinSet::new();
            let mut accepted = 0;
            let mut failure = None;
            loop {
                let next = tokio::select! {
                    biased;
                    _ = &mut stopped => break,
                    result = handlers.join_next(), if !handlers.is_empty() => {
                        if !matches!(result, Some(Ok(Ok(())))) { failure = Some("owned TLS exchange failed"); break; }
                        continue;
                    },
                    result = api.accept(), if accepted < 8 => (result, Origin::Api, api_tls.clone()),
                    result = media.accept(), if accepted < 8 => (result, Origin::Media, media_tls.clone()),
                };
                let socket = match next.0 {
                    Ok((socket, _)) => socket,
                    Err(_) => {
                        failure = Some("owned TLS accept failed");
                        break;
                    }
                };
                accepted += 1;
                let state = Handler {
                    origin: next.1,
                    ..state.clone()
                };
                handlers.spawn(async move {
                    tokio::time::timeout(Duration::from_secs(5), async {
                        let socket = next
                            .2
                            .accept(socket)
                            .await
                            .map_err(|_| "owned TLS handshake failed")?;
                        let name = socket
                            .get_ref()
                            .1
                            .server_name()
                            .unwrap_or_default()
                            .to_owned();
                        let app = Router::new()
                            .fallback(exchange)
                            .layer(Extension(name))
                            .with_state(state);
                        hyper::server::conn::http1::Builder::new()
                            .serve_connection(TokioIo::new(socket), TowerToHyperService::new(app))
                            .await
                            .map_err(|_| "owned HTTP exchange failed")
                    })
                    .await
                    .map_err(|_| "owned TLS exchange exceeded its deadline")?
                });
            }
            while let Some(result) = handlers.join_next().await {
                if !matches!(result, Ok(Ok(()))) {
                    failure = Some("owned TLS exchange failed during joined shutdown");
                }
            }
            failure.map_or(Ok(()), Err)
        });
        Self {
            ca: ca_der,
            api_address,
            media_address,
            exchanges,
            file_path,
            stop: Some(stop),
            task: Some(task),
        }
    }

    pub fn ca_der(&self) -> &[u8] {
        &self.ca
    }

    #[allow(dead_code)]
    pub fn replace_selected_file_path(&self, file_path: &str) {
        assert!(file_path.starts_with('/') && file_path.len() <= 4096);
        *self
            .file_path
            .lock()
            .expect("owned provider file path must lock") = file_path.to_owned();
    }

    pub fn counts(&self) -> Counts {
        let exchanges = self.exchanges.lock().expect("owned exchanges must lock");
        Counts {
            observations: exchanges
                .iter()
                .filter(|request| request.origin == Origin::Api && request.method == "GET")
                .count(),
            posts: exchanges
                .iter()
                .filter(|request| request.origin == Origin::Api && request.method == "POST")
                .count(),
            media: exchanges
                .iter()
                .filter(|request| request.origin == Origin::Media)
                .count(),
        }
    }

    pub async fn close(mut self) -> Result<Vec<Exchange>, &'static str> {
        if let Some(stop) = self.stop.take() {
            let _ = stop.send(());
        }
        if let Some(mut task) = self.task.take() {
            match tokio::time::timeout(Duration::from_secs(7), &mut task).await {
                Ok(result) => result.map_err(|_| "owned TLS fixture panicked")??,
                Err(_) => {
                    task.abort();
                    let _ = task.await;
                    return Err("owned TLS fixture failed to join shutdown");
                }
            }
        }
        let mut exchanges = self
            .exchanges
            .lock()
            .map_err(|_| "owned exchanges lock failed")?;
        let captured = std::mem::take(&mut *exchanges);
        drop(exchanges);
        Ok(captured)
    }
}

impl Drop for Provider {
    fn drop(&mut self) {
        if let Some(stop) = self.stop.take() {
            let _ = stop.send(());
        }
        if let Some(task) = self.task.take() {
            task.abort();
        }
    }
}

async fn exchange(
    State(state): State<Handler>,
    Extension(server_name): Extension<String>,
    request: Request,
) -> Response {
    let (parts, body) = http::capture_request(request).await;
    let method = parts.method.as_str().to_owned();
    let target = parts.uri.to_string();
    {
        let mut exchanges = state.exchanges.lock().expect("owned exchanges must lock");
        assert!(exchanges.len() < 8, "owned exchanges exceeded their bound");
        exchanges.push(Exchange {
            origin: state.origin,
            server_name,
            method: method.clone(),
            target: target.clone(),
            headers: parts.headers,
            body,
        });
    }
    let mut response = match (state.origin, method.as_str(), target.as_str()) {
        (Origin::Api, "GET", target) if target == format!("/rest/1.0/torrents/info/{TORRENT}") => {
            let file_path = state
                .file_path
                .lock()
                .expect("owned provider file path must lock")
                .clone();
            Json(json!({
                "id": TORRENT, "hash": HASH, "bytes": state.audio.len(),
                "progress": 100, "status": "downloaded",
                "files": [{"id": 7, "path": file_path, "bytes": state.audio.len(), "selected": 1}],
                "links": [HOSTER_LINK]
            }))
            .into_response()
        }
        (Origin::Api, "POST", "/rest/1.0/unrestrict/link") => Json(json!({
            "filesize": state.audio.len(), "link": HOSTER_LINK,
            "download": format!("https://{MEDIA_HOST}{MEDIA_TARGET}")
        }))
        .into_response(),
        (Origin::Media, "GET", MEDIA_TARGET) => {
            let headers = [
                ("content-type", state.content_type),
                ("set-cookie", "private-media-cookie-canary"),
                (
                    "content-disposition",
                    "attachment; filename=private-media-name-canary.wav",
                ),
                ("accept-ranges", "bytes"),
                ("etag", "private-media-validator-canary"),
            ];
            match &state.media_gate {
                Some(gate) => (headers, gate.body(state.audio.clone())).into_response(),
                None => (headers, state.audio.as_ref().clone()).into_response(),
            }
        }
        _ => StatusCode::NOT_FOUND.into_response(),
    };
    response
        .headers_mut()
        .insert("connection", HeaderValue::from_static("close"));
    response
}
