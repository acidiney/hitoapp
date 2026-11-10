#![allow(dead_code)]
use axum::{
    Json, Router,
    body::to_bytes,
    extract::{Request, State},
    http::{StatusCode, header},
    response::{IntoResponse, Response},
};
use hito_audio::test_support::{capture::Capture, continuity::counter};
use hito_core::{
    Actor, AdoptProviderRecording, Credentials, InstallPlugin, OfferPluginService, PrepareSource,
    Search, Services, test_support::support,
};
use hito_playback::{
    Player, PlayerAction, PlayerCommand, PlayerOwner, PlayerPosition, PlayerSnapshot,
};
use serde_json::{Value, json};
use std::{
    io::Write,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    time::Duration,
};
use tokio::sync::watch;
use uuid::Uuid;

pub const RATE: u32 = 48_000;
pub const SECOND_RECORDING: &str = "deferred-second";

#[derive(Clone, Copy)]
pub enum Mode {
    Ready(Duration),
    Never,
    Failed,
}

struct ProviderState {
    mode: Mode,
    prepared: AtomicBool,
    begins: AtomicUsize,
    polls: AtomicUsize,
    audio: Vec<u8>,
}

pub struct Provider {
    pub url: String,
    state: Arc<ProviderState>,
    task: tokio::task::JoinHandle<()>,
}

impl Provider {
    async fn start(mode: Mode, audio: Vec<u8>) -> Self {
        let state = Arc::new(ProviderState {
            mode,
            prepared: AtomicBool::new(false),
            begins: AtomicUsize::new(0),
            polls: AtomicUsize::new(0),
            audio,
        });
        let router = Router::new().fallback(exchange).with_state(state.clone());
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}/", listener.local_addr().unwrap());
        let task = tokio::spawn(async move {
            axum::serve(listener, router).await.unwrap();
        });
        Self { url, state, task }
    }

    pub fn begins(&self) -> usize {
        self.state.begins.load(Ordering::SeqCst)
    }

    pub fn polls(&self) -> usize {
        self.state.polls.load(Ordering::SeqCst)
    }
}

impl Drop for Provider {
    fn drop(&mut self) {
        self.task.abort();
    }
}

async fn exchange(State(state): State<Arc<ProviderState>>, request: Request) -> Response {
    let path = request.uri().path().to_owned();
    if path == "/v1/audio" {
        return Response::builder()
            .header(header::CONTENT_TYPE, "audio/wav")
            .header(header::CONTENT_LENGTH, state.audio.len())
            .body(axum::body::Body::from(state.audio.clone()))
            .unwrap();
    }
    let bytes = to_bytes(request.into_body(), 32768).await.unwrap();
    let body: Value = if bytes.is_empty() {
        Value::Null
    } else {
        serde_json::from_slice(&bytes).unwrap()
    };
    let recording = body["recording_id"].as_str().unwrap_or_default().to_owned();
    let echo = |extra: Value| {
        let mut response = json!({
            "operation_id": body["operation_id"],
            "recording_id": body["recording_id"],
            "source_id": body["source_id"],
        });
        for (key, value) in extra.as_object().unwrap() {
            response[key] = value.clone();
        }
        response
    };
    let response = match path.as_str() {
        "/v1/manifest" => json!({
            "id": "org.hito.deferred-fixture", "name": "Owned deferred provider",
            "protocol_version": 1, "capabilities": ["sources", "source_preparation"]
        }),
        "/v1/recordings/detail" => json!({
            "id": recording, "title": "Owned recording", "artist": "Owned artist"
        }),
        "/v1/sources" => json!({
            "items": [{
                "id": format!("{recording}-file"),
                "label": "Owned deferred file",
                "ready": recording == SECOND_RECORDING && state.prepared.load(Ordering::SeqCst)
            }]
        }),
        "/v1/sources/prepare" => {
            state.begins.fetch_add(1, Ordering::SeqCst);
            let status = match state.mode {
                Mode::Ready(delay) => {
                    tokio::time::sleep(delay).await;
                    state.prepared.store(true, Ordering::SeqCst);
                    json!({"phase": "ready"})
                }
                Mode::Never => json!({"phase": "preparing", "poll_after_seconds": 5}),
                Mode::Failed => json!({"phase": "failed", "reason": "source_unavailable"}),
            };
            echo(
                json!({"submission": {"outcome": "accepted", "handle": "owned-handle", "status": status}}),
            )
        }
        "/v1/sources/preparation/status" => {
            state.polls.fetch_add(1, Ordering::SeqCst);
            let status = if state.prepared.load(Ordering::SeqCst) {
                json!({"phase": "ready"})
            } else {
                json!({"phase": "preparing", "poll_after_seconds": 5})
            };
            echo(json!({"handle": body["handle"], "status": status}))
        }
        _ => return StatusCode::NOT_FOUND.into_response(),
    };
    Json(response).into_response()
}

pub fn counter_wav(start: u64, frames: u32) -> Vec<u8> {
    let size = frames * 4;
    let mut bytes = Vec::with_capacity(44 + size as usize);
    bytes.extend_from_slice(b"RIFF");
    bytes.extend_from_slice(&(size + 36).to_le_bytes());
    bytes.extend_from_slice(b"WAVEfmt ");
    bytes.extend_from_slice(&16_u32.to_le_bytes());
    bytes.extend_from_slice(&1_u16.to_le_bytes());
    bytes.extend_from_slice(&2_u16.to_le_bytes());
    bytes.extend_from_slice(&RATE.to_le_bytes());
    bytes.extend_from_slice(&(RATE * 4).to_le_bytes());
    bytes.extend_from_slice(&4_u16.to_le_bytes());
    bytes.extend_from_slice(&16_u16.to_le_bytes());
    bytes.extend_from_slice(b"data");
    bytes.extend_from_slice(&size.to_le_bytes());
    for frame in 0..frames {
        let sample = counter(start + u64::from(frame)).to_le_bytes();
        bytes.extend_from_slice(&sample);
        bytes.extend_from_slice(&sample);
    }
    bytes
}

#[derive(Clone, Default)]
pub struct Events {
    bytes: Arc<Mutex<Vec<u8>>>,
}

impl Write for Events {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.bytes.lock().unwrap().extend_from_slice(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

impl Events {
    pub fn count(&self, event: &str) -> usize {
        let log = self.bytes.lock().unwrap();
        String::from_utf8_lossy(&log)
            .matches(&format!("\"event\":\"{event}\""))
            .count()
    }

    pub fn text(&self) -> String {
        String::from_utf8_lossy(&self.bytes.lock().unwrap()).into_owned()
    }
}

pub struct Deferred {
    pub directory: tempfile::TempDir,
    pub services: Services,
    pub actor: Actor,
    pub token: String,
    pub provider: Provider,
    pub installation: String,
    pub first: String,
    pub second: String,
    pub frames: (u32, u32),
}

impl Deferred {
    pub async fn start(first_frames: u32, second_frames: u32, mode: Mode) -> Self {
        let provider =
            Provider::start(mode, counter_wav(u64::from(first_frames), second_frames)).await;
        let (directory, services, admin, library, music) = support::library("deferred").await;
        std::fs::write(music.join("first.wav"), counter_wav(0, first_frames)).unwrap();
        support::scan(&services, &admin, &library).await;
        let session = services
            .identity
            .login(Credentials {
                username: "admin".into(),
                password: "indexing test password".into(),
            })
            .await
            .unwrap();
        let actor = services
            .identity
            .authenticate(&session.token)
            .await
            .unwrap();
        let service = services
            .plugins
            .offer_service(
                &actor,
                OfferPluginService {
                    name: "Deferred".into(),
                    base_url: provider.url.clone(),
                },
            )
            .await
            .unwrap();
        let installation = services
            .plugins
            .install(
                &actor,
                InstallPlugin {
                    accept_manifest_digest: None,
                    service_id: service.id,
                    service_revision: 1,
                    bearer_token: Some("private-deferred-token".into()),
                },
            )
            .await
            .unwrap()
            .id;
        let second = Self::adopt_with(&services, &actor, &installation, SECOND_RECORDING).await;
        let first = services
            .catalog
            .recordings(&actor, Search::default())
            .await
            .unwrap()
            .items
            .into_iter()
            .find(|recording| recording.id != second)
            .expect("the local recording")
            .id;
        Self {
            directory,
            services,
            actor,
            token: session.token,
            provider,
            installation,
            first,
            second,
            frames: (first_frames, second_frames),
        }
    }

    async fn adopt_with(
        services: &Services,
        actor: &Actor,
        installation: &str,
        provider_recording: &str,
    ) -> String {
        services
            .catalog
            .adopt_provider(
                actor,
                installation,
                AdoptProviderRecording {
                    installation_revision: 1,
                    provider_recording_id: provider_recording.into(),
                },
            )
            .await
            .unwrap()
            .id
    }

    pub async fn saturate_preparations(&self, count: usize) {
        for index in 0..count {
            let recording = Self::adopt_with(
                &self.services,
                &self.actor,
                &self.installation,
                &format!("deferred-extra-{index}"),
            )
            .await;
            let source = self
                .services
                .catalog
                .sources(&self.actor, &recording)
                .await
                .unwrap()
                .into_iter()
                .find(|source| source.can_prepare)
                .expect("a preparable source")
                .id;
            self.services
                .catalog
                .prepare_source(
                    &self.actor,
                    &recording,
                    &source,
                    PrepareSource {
                        request_id: Uuid::new_v4(),
                    },
                )
                .await
                .unwrap();
        }
    }

    pub async fn count(&self, table: &str) -> i64 {
        let pool = sqlx::sqlite::SqlitePoolOptions::new()
            .max_connections(1)
            .connect_with(
                sqlx::sqlite::SqliteConnectOptions::new()
                    .filename(self.directory.path().join("hito.db"))
                    .read_only(true),
            )
            .await
            .unwrap();
        let count = sqlx::query_scalar(&format!("SELECT count(*) FROM {table}"))
            .fetch_one(&pool)
            .await
            .unwrap();
        pool.close().await;
        count
    }

    pub async fn download_rows(&self) -> i64 {
        let mut total = 0;
        for table in ["download_files", "download_intents", "download_jobs"] {
            total += self.count(table).await;
        }
        total
    }
}

pub struct Run {
    pub f: Deferred,
    pub player: Player,
    pub owner: PlayerOwner,
    pub lease: Uuid,
    pub state: watch::Receiver<PlayerSnapshot>,
    pub position: watch::Receiver<PlayerPosition>,
    pub events: Events,
    pub capture: Capture,
    sequence: u64,
    worker: Option<tokio::task::JoinHandle<()>>,
}

impl Run {
    pub async fn start(f: Deferred, worker: bool, queue: impl Fn(&Deferred) -> Vec<&str>) -> Self {
        let events = Events::default();
        let writer = events.clone();
        hito_testkit::logs::anchor_callsite_interest();
        let dispatch = tracing::Dispatch::new(
            tracing_subscriber::fmt()
                .json()
                .with_ansi(false)
                .with_max_level(tracing::Level::DEBUG)
                .with_writer(move || writer.clone())
                .finish(),
        );
        let capture = Capture::open();
        let (player, owner) =
            tracing::dispatcher::with_default(&dispatch, || Player::native(f.services.clone()));
        let lease = player
            .claim(&f.actor, player.inspect(&f.actor).await.unwrap().epoch)
            .await
            .unwrap()
            .lease;
        let state = player.subscribe(&f.actor, lease).await.unwrap();
        let position = player.subscribe_position(&f.actor, lease).await.unwrap();
        let recordings = queue(&f)
            .into_iter()
            .map(|id| Uuid::parse_str(id).unwrap())
            .collect();
        let worker = worker.then(|| {
            let catalog = f.services.catalog.clone();
            tokio::spawn(async move {
                loop {
                    if !catalog
                        .run_next_source_preparation_until(std::future::pending())
                        .await
                        .unwrap_or(false)
                    {
                        tokio::time::sleep(Duration::from_millis(50)).await;
                    }
                }
            })
        });
        let mut run = Self {
            f,
            player,
            owner,
            lease,
            state,
            position,
            events,
            capture,
            sequence: 0,
            worker,
        };
        run.send(PlayerAction::Replace {
            recordings,
            start: 0,
        })
        .await;
        run
    }

    pub async fn send(&mut self, action: PlayerAction) {
        let revision = self.state.borrow().revision;
        self.sequence += 1;
        self.player
            .submit(
                &self.f.actor,
                PlayerCommand {
                    id: Uuid::new_v4(),
                    lease: self.lease,
                    sequence: self.sequence,
                    revision,
                    action,
                },
            )
            .await
            .unwrap();
    }

    pub async fn entries(&mut self, count: usize) -> Vec<Uuid> {
        let revision = self.state.borrow().revision;
        self.player
            .queue(&self.f.actor, self.lease, revision, 0, count)
            .await
            .unwrap()
            .entries
            .into_iter()
            .map(|entry| entry.id)
            .collect()
    }

    pub async fn position_reaches(&mut self, milliseconds: u64) {
        tokio::time::timeout(
            Duration::from_secs(10),
            self.position
                .wait_for(|position| position.position_ms >= milliseconds),
        )
        .await
        .unwrap()
        .unwrap();
    }

    pub async fn queue_finishes(&mut self, seconds: u64) {
        tokio::time::timeout(
            Duration::from_secs(seconds),
            self.state.wait_for(|state| state.current.is_none()),
        )
        .await
        .unwrap()
        .unwrap();
    }

    pub async fn until(&self, what: &str, mut ready: impl FnMut() -> bool) {
        tokio::time::timeout(Duration::from_secs(30), async {
            while !ready() {
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
        })
        .await
        .unwrap_or_else(|_| panic!("timed out waiting for {what}"));
    }

    pub fn phases(&self) -> Arc<Mutex<Vec<String>>> {
        let seen = Arc::new(Mutex::new(Vec::new()));
        let mut state = self.state.clone();
        let sink = seen.clone();
        tokio::spawn(async move {
            while state.changed().await.is_ok() {
                let phase = format!("{:?}", state.borrow_and_update().phase);
                let mut seen = sink.lock().unwrap();
                if seen.last() != Some(&phase) {
                    seen.push(phase);
                }
            }
        });
        seen
    }

    pub async fn heard(self) -> (Vec<f32>, Events, Deferred) {
        if let Some(worker) = &self.worker {
            worker.abort();
        }
        self.owner.close().await.unwrap();
        self.capture.after(self.capture.mark(), 0.3);
        let captured = self.capture.all();
        if let Some(root) = std::env::var_os("HITO_CAPTURE_DUMP") {
            let thread = std::thread::current();
            let case = thread.name().unwrap_or("deferred");
            hito_audio::test_support::continuity::dump(case, &captured);
            std::fs::write(
                std::path::Path::new(&root).join(format!("{case}.json")),
                self.events.text(),
            )
            .unwrap();
        }
        (captured, self.events, self.f)
    }
}
