use axum::{
    Router,
    body::{Body, Bytes, to_bytes},
    extract::{Request, State},
    http::{
        HeaderMap, Method, StatusCode,
        header::{CONTENT_LENGTH, CONTENT_TYPE},
    },
    response::{IntoResponse, Response},
};
use hito_plugin_sdk::{
    MANIFEST_PATH, Manifest,
    server::{AuthFailure, PROTOCOL_HEADER, ServiceAuth, require_protocol},
};
use serde_json::Value;
use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio::{
    sync::{Semaphore, oneshot, watch},
    task::{JoinHandle, JoinSet},
};

const PREFIX: &str = "/prefix/";
const BODY_LIMIT: usize = 1 << 20;
const WAIT_LIMIT: Duration = Duration::from_secs(10);

#[derive(Clone)]
pub enum Reply {
    Json(Value),
    Refuse,
    Status(StatusCode),
    Raw {
        status: StatusCode,
        headers: HeaderMap,
        body: Vec<u8>,
        chunked: bool,
    },
}

#[derive(Clone)]
pub enum Fault {
    Status(StatusCode),
    Delay(Duration),
    Malformed,
    DropConnection,
    Hold(Arc<Gate>),
    /// Parks the next accepted request in a worker that outlives its connection.
    HoldNext(Arc<Gate>),
    /// Sends the first `prefix` body bytes, then waits for the gate to be released.
    StallBody {
        gate: Arc<Gate>,
        prefix: usize,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Handled {
    Yes,
    No,
    Rejected,
    Refused,
}

#[derive(Debug, Clone)]
pub struct RecordedRequest {
    pub operation: String,
    pub method: Method,
    pub query: Option<String>,
    pub headers: HeaderMap,
    pub body: Vec<u8>,
    pub handled: Handled,
}

impl RecordedRequest {
    pub fn json(&self) -> Value {
        serde_json::from_slice(&self.body).unwrap_or(Value::Null)
    }

    pub fn has_authorization(&self) -> bool {
        self.headers.contains_key("authorization")
    }

    pub fn protocol(&self) -> Option<String> {
        self.headers
            .get(PROTOCOL_HEADER)
            .and_then(|value| value.to_str().ok())
            .map(str::to_owned)
    }
}

pub struct Gate {
    entered: Semaphore,
    release: Semaphore,
    retired: Semaphore,
}

impl Gate {
    fn new() -> Arc<Self> {
        Arc::new(Self {
            entered: Semaphore::new(0),
            release: Semaphore::new(0),
            retired: Semaphore::new(0),
        })
    }

    pub fn has_retired(&self) -> bool {
        self.retired.available_permits() != 0
    }

    pub async fn entered(&self) {
        assert!(
            self.entered_within(WAIT_LIMIT).await,
            "no request reached the gate within {WAIT_LIMIT:?}"
        );
    }

    pub async fn entered_within(&self, limit: Duration) -> bool {
        take(&self.entered, limit).await
    }

    pub fn release(&self) {
        self.release.add_permits(1);
    }

    pub fn open(&self) {
        self.release.close();
    }

    pub async fn retired(&self) {
        assert!(
            self.retired_within(WAIT_LIMIT).await,
            "no held request retired within {WAIT_LIMIT:?}"
        );
    }

    pub async fn retired_within(&self, limit: Duration) -> bool {
        take(&self.retired, limit).await
    }

    async fn wait(self: Arc<Self>, limit: Duration) -> bool {
        let _retiring = Retiring(self.clone());
        self.entered.add_permits(1);
        wait_release(&self.release, limit).await
    }
}

async fn take(semaphore: &Semaphore, limit: Duration) -> bool {
    match tokio::time::timeout(limit, semaphore.acquire()).await {
        Ok(Ok(permit)) => {
            permit.forget();
            true
        }
        _ => false,
    }
}

async fn wait_release(release: &Semaphore, limit: Duration) -> bool {
    match tokio::time::timeout(limit, release.acquire()).await {
        Ok(Ok(permit)) => {
            permit.forget();
            true
        }
        Ok(Err(_)) => true,
        Err(_) => false,
    }
}

struct Retiring(Arc<Gate>);

impl Drop for Retiring {
    fn drop(&mut self) {
        self.0.retired.add_permits(1);
    }
}

type Handler = Arc<dyn Fn(&RecordedRequest) -> Reply + Send + Sync>;

struct Route {
    method: Method,
    operation: String,
    handler: Handler,
}

struct Shared {
    routes: Mutex<Vec<Route>>,
    faults: Mutex<HashMap<String, Fault>>,
    workers: Mutex<JoinSet<()>>,
    log: Mutex<Vec<RecordedRequest>>,
    logged: watch::Sender<usize>,
    auth: Mutex<Option<ServiceAuth>>,
    require_protocol: bool,
    strict_limit: Option<usize>,
    require_json: bool,
    hold_limit: Mutex<Duration>,
}

pub struct FakePluginBuilder {
    routes: Vec<Route>,
    auth: Option<ServiceAuth>,
    require_protocol: bool,
    strict_limit: Option<usize>,
    require_json: bool,
}

pub struct FakePlugin {
    pub url: String,
    shared: Arc<Shared>,
    stop: Option<oneshot::Sender<()>>,
    task: Option<JoinHandle<bool>>,
}

impl FakePlugin {
    pub fn builder(manifest: Manifest) -> FakePluginBuilder {
        let manifest = serde_json::to_value(manifest).expect("manifest must serialize");
        FakePluginBuilder {
            routes: Vec::new(),
            auth: None,
            require_protocol: false,
            strict_limit: None,
            require_json: false,
        }
        .get(MANIFEST_PATH, move |_| Reply::Json(manifest.clone()))
    }

    pub fn requests(&self) -> Vec<RecordedRequest> {
        self.shared.log.lock().unwrap().clone()
    }

    pub fn requests_for(&self, operation: &str) -> Vec<RecordedRequest> {
        self.requests()
            .into_iter()
            .filter(|request| request.operation == operation)
            .collect()
    }

    pub async fn wait_for_requests(&self, count: usize) {
        let mut logged = self.shared.logged.subscribe();
        tokio::time::timeout(WAIT_LIMIT, logged.wait_for(|logged| *logged >= count))
            .await
            .unwrap_or_else(|_| {
                panic!("the fake plugin did not log {count} requests within {WAIT_LIMIT:?}")
            })
            .expect("the fake plugin log must stay open");
    }

    pub fn unexpected(&self) -> usize {
        self.requests()
            .iter()
            .filter(|request| matches!(request.handled, Handled::No | Handled::Refused))
            .count()
    }

    pub fn unmatched(&self) -> usize {
        self.requests()
            .iter()
            .filter(|request| request.handled == Handled::No)
            .count()
    }

    pub fn on(
        &self,
        operation: &str,
        handler: impl Fn(&RecordedRequest) -> Reply + Send + Sync + 'static,
    ) {
        let mut routes = self.shared.routes.lock().unwrap();
        let route = routes
            .iter_mut()
            .find(|route| route.operation == operation)
            .unwrap_or_else(|| panic!("the fake plugin has no operation {operation}"));
        route.handler = Arc::new(handler);
    }

    pub fn reply(&self, operation: &str, reply: Reply) {
        self.on(operation, move |_| reply.clone());
    }

    pub fn require_bearer(&self, token: &str) {
        *self.shared.auth.lock().unwrap() =
            Some(ServiceAuth::new(token).expect("the fake plugin token must be valid"));
    }

    pub fn fault(&self, operation: &str, fault: Fault) {
        let previous = self
            .shared
            .faults
            .lock()
            .unwrap()
            .insert(operation.to_owned(), fault);
        assert!(
            previous.is_none(),
            "{operation} already has a fault; clear it first"
        );
    }

    pub fn set_hold_limit(&self, limit: Duration) {
        *self.shared.hold_limit.lock().unwrap() = limit;
    }

    pub fn clear_fault(&self, operation: &str) {
        self.shared.faults.lock().unwrap().remove(operation);
    }

    pub fn hold(&self, operation: &str) -> Arc<Gate> {
        let gate = Gate::new();
        self.fault(operation, Fault::Hold(gate.clone()));
        gate
    }

    pub fn stall_body(&self, operation: &str, prefix: usize) -> Arc<Gate> {
        let gate = Gate::new();
        self.fault(
            operation,
            Fault::StallBody {
                gate: gate.clone(),
                prefix,
            },
        );
        gate
    }

    pub fn hold_next(&self, operation: &str) -> Arc<Gate> {
        let gate = Gate::new();
        self.fault(operation, Fault::HoldNext(gate.clone()));
        gate
    }

    pub async fn finish(&mut self) -> bool {
        let mut workers = std::mem::take(&mut *self.shared.workers.lock().unwrap());
        workers.abort_all();
        while workers.join_next().await.is_some() {}
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

impl Drop for FakePlugin {
    fn drop(&mut self) {
        self.shared.workers.lock().unwrap().abort_all();
        if let Some(stop) = self.stop.take() {
            let _ = stop.send(());
        }
        if let Some(task) = self.task.take() {
            task.abort();
        }
    }
}

impl FakePluginBuilder {
    pub fn get(
        self,
        operation: &str,
        handler: impl Fn(&RecordedRequest) -> Reply + Send + Sync + 'static,
    ) -> Self {
        self.route(Method::GET, operation, handler)
    }

    pub fn post(
        self,
        operation: &str,
        handler: impl Fn(&RecordedRequest) -> Reply + Send + Sync + 'static,
    ) -> Self {
        self.route(Method::POST, operation, handler)
    }

    pub fn require_bearer(mut self, token: &str) -> Self {
        self.auth = Some(ServiceAuth::new(token).expect("the fake plugin token must be valid"));
        self
    }

    pub fn require_protocol(mut self) -> Self {
        self.require_protocol = true;
        self
    }

    pub fn require_json(mut self) -> Self {
        self.require_json = true;
        self
    }

    pub fn strict(mut self, request_limit: usize) -> Self {
        self.strict_limit = Some(request_limit);
        self
    }

    pub async fn start(self) -> FakePlugin {
        let shared = Arc::new(Shared {
            routes: Mutex::new(self.routes),
            faults: Mutex::default(),
            workers: Mutex::default(),
            log: Mutex::default(),
            logged: watch::channel(0).0,
            auth: Mutex::new(self.auth),
            require_protocol: self.require_protocol,
            strict_limit: self.strict_limit,
            require_json: self.require_json,
            hold_limit: Mutex::new(WAIT_LIMIT),
        });
        let router = Router::new().fallback(dispatch).with_state(shared.clone());
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("the fake plugin listener must bind");
        let url = format!(
            "http://{}{PREFIX}",
            listener.local_addr().expect("the fake plugin address")
        );
        let (stop, stopped) = oneshot::channel();
        let task = tokio::spawn(async move {
            axum::serve(listener, router)
                .with_graceful_shutdown(async {
                    let _ = stopped.await;
                })
                .await
                .is_ok()
        });
        FakePlugin {
            url,
            shared,
            stop: Some(stop),
            task: Some(task),
        }
    }

    fn route(
        mut self,
        method: Method,
        operation: &str,
        handler: impl Fn(&RecordedRequest) -> Reply + Send + Sync + 'static,
    ) -> Self {
        self.routes.push(Route {
            method,
            operation: operation.to_owned(),
            handler: Arc::new(handler),
        });
        self
    }
}

async fn dispatch(State(shared): State<Arc<Shared>>, request: Request) -> Response {
    let (parts, body) = request.into_parts();
    let Ok(body) = to_bytes(body, BODY_LIMIT).await else {
        return StatusCode::PAYLOAD_TOO_LARGE.into_response();
    };
    let path = parts.uri.path();
    let operation = path
        .strip_prefix(PREFIX)
        .unwrap_or_else(|| path.trim_start_matches('/'))
        .to_owned();
    let handler = shared
        .routes
        .lock()
        .unwrap()
        .iter()
        .find(|route| route.operation == operation && route.method == parts.method)
        .map(|route| route.handler.clone());
    let rejection = authorize(&shared, &parts.headers);
    let handled = match (&handler, &rejection) {
        (None, _) => Handled::No,
        (Some(_), Err(_)) => Handled::Rejected,
        (Some(_), Ok(())) => Handled::Yes,
    };
    let mut recorded = RecordedRequest {
        operation: operation.clone(),
        method: parts.method,
        query: parts.uri.query().map(str::to_owned),
        headers: parts.headers,
        body: body.to_vec(),
        handled,
    };
    let mut refusal = None;
    let index = {
        let mut log = shared.log.lock().unwrap();
        if handled == Handled::Yes {
            refusal = refusal_status(&shared, log.len(), &recorded);
            if refusal.is_some() {
                recorded.handled = Handled::Refused;
            }
        }
        log.push(recorded.clone());
        log.len() - 1
    };
    let Some(handler) = handler else {
        announce(&shared);
        return StatusCode::NOT_FOUND.into_response();
    };
    match rejection {
        Err(AuthFailure::Unauthorized) => {
            announce(&shared);
            return StatusCode::UNAUTHORIZED.into_response();
        }
        Err(AuthFailure::InvalidProtocol) => {
            announce(&shared);
            return StatusCode::BAD_REQUEST.into_response();
        }
        Ok(()) => {}
    }
    if let Some(status) = refusal {
        announce(&shared);
        return status.into_response();
    }
    let fault = shared.faults.lock().unwrap().get(&operation).cloned();
    match fault {
        Some(Fault::Status(status)) => {
            announce(&shared);
            return status.into_response();
        }
        Some(Fault::Malformed) => {
            announce(&shared);
            return (
                StatusCode::OK,
                [(CONTENT_TYPE, "application/json")],
                "{not json",
            )
                .into_response();
        }
        Some(Fault::DropConnection) => {
            announce(&shared);
            return dropped_connection();
        }
        Some(Fault::Delay(_) | Fault::Hold(_) | Fault::HoldNext(_) | Fault::StallBody { .. })
        | None => {}
    }
    let reply = handler(&recorded);
    if matches!(reply, Reply::Refuse) {
        shared.log.lock().unwrap()[index].handled = Handled::Refused;
    }
    announce(&shared);
    if matches!(reply, Reply::Refuse) {
        return into_response(reply);
    }
    let limit = *shared.hold_limit.lock().unwrap();
    if let Some(Fault::StallBody { gate, prefix }) = fault {
        return stalled_response(reply, prefix, gate, limit);
    }
    let released = match fault {
        Some(Fault::Delay(delay)) => {
            tokio::time::sleep(delay).await;
            true
        }
        Some(Fault::Hold(gate)) => gate.wait(limit).await,
        Some(Fault::HoldNext(gate)) if claim_next(&shared, &operation, &gate) => {
            let (done, completed) = oneshot::channel();
            {
                let mut workers = shared.workers.lock().unwrap();
                while workers.try_join_next().is_some() {}
                workers.spawn(async move {
                    let _ = done.send(gate.wait(limit).await);
                });
            }
            completed.await.unwrap_or(false)
        }
        _ => true,
    };
    if !released {
        return StatusCode::GATEWAY_TIMEOUT.into_response();
    }
    into_response(reply)
}

fn announce(shared: &Shared) {
    let count = shared.log.lock().unwrap().len();
    shared.logged.send_replace(count);
}

fn claim_next(shared: &Shared, operation: &str, gate: &Arc<Gate>) -> bool {
    let mut faults = shared.faults.lock().unwrap();
    match faults.get(operation) {
        Some(Fault::HoldNext(held)) if Arc::ptr_eq(held, gate) => {
            faults.remove(operation);
            true
        }
        _ => false,
    }
}

fn refusal_status(shared: &Shared, prior: usize, request: &RecordedRequest) -> Option<StatusCode> {
    let json_body = serde_json::from_slice::<Value>(&request.body).is_ok();
    if shared.require_json && request.method == Method::POST {
        let json_type = request
            .headers
            .get(CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
            .is_some_and(|value| {
                value
                    .split(';')
                    .next()
                    .is_some_and(|mime| mime.trim().eq_ignore_ascii_case("application/json"))
            });
        if !json_type {
            return Some(StatusCode::UNSUPPORTED_MEDIA_TYPE);
        }
        if !json_body {
            return Some(StatusCode::BAD_REQUEST);
        }
    }
    let strict = shared.strict_limit.is_some_and(|limit| {
        prior >= limit || request.query.is_some() || (!request.body.is_empty() && !json_body)
    });
    strict.then_some(StatusCode::BAD_REQUEST)
}

fn authorize(shared: &Shared, headers: &HeaderMap) -> Result<(), AuthFailure> {
    match shared.auth.lock().unwrap().as_ref() {
        Some(auth) => auth.check(headers),
        None if shared.require_protocol => require_protocol(headers),
        None => Ok(()),
    }
}

fn parts(reply: Reply) -> (StatusCode, HeaderMap, Vec<u8>, bool) {
    match reply {
        Reply::Json(value) => (
            StatusCode::OK,
            HeaderMap::from_iter([(CONTENT_TYPE, "application/json".parse().unwrap())]),
            serde_json::to_vec(&value).unwrap(),
            false,
        ),
        Reply::Status(status) => (status, HeaderMap::new(), Vec::new(), false),
        Reply::Refuse => (StatusCode::BAD_REQUEST, HeaderMap::new(), Vec::new(), false),
        Reply::Raw {
            status,
            headers,
            body,
            chunked,
        } => (status, headers, body, chunked),
    }
}

fn into_response(reply: Reply) -> Response {
    let (status, headers, body, chunked) = parts(reply);
    let body = if chunked {
        Body::from_stream(futures_util::stream::once(async move {
            Ok::<_, std::convert::Infallible>(Bytes::from(body))
        }))
    } else {
        Body::from(body)
    };
    (status, headers, body).into_response()
}

struct StalledBody {
    first: Option<Bytes>,
    rest: Option<Bytes>,
    gate: Arc<Gate>,
    limit: Duration,
    _retiring: Retiring,
}

fn stalled_response(reply: Reply, prefix: usize, gate: Arc<Gate>, limit: Duration) -> Response {
    let (status, mut headers, bytes, _) = parts(reply);
    headers.insert(CONTENT_LENGTH, bytes.len().into());
    let mut first = Bytes::from(bytes);
    let rest = first.split_off(prefix.min(first.len()));
    let body = StalledBody {
        first: Some(first),
        rest: Some(rest),
        limit,
        _retiring: Retiring(gate.clone()),
        gate,
    };
    let stream = futures_util::stream::unfold(body, |mut body| async move {
        if let Some(first) = body.first.take() {
            body.gate.entered.add_permits(1);
            return Some((Ok::<_, std::io::Error>(first), body));
        }
        let rest = body.rest.take()?;
        if !wait_release(&body.gate.release, body.limit).await {
            return Some((
                Err(std::io::Error::other("the stalled body timed out")),
                body,
            ));
        }
        Some((Ok(rest), body))
    });
    (status, headers, Body::from_stream(stream)).into_response()
}

fn dropped_connection() -> Response {
    let stream = futures_util::stream::iter([
        Ok(Bytes::from_static(b"{")),
        Err(std::io::Error::other(
            "the fake plugin dropped the connection",
        )),
    ]);
    (
        StatusCode::OK,
        [(CONTENT_TYPE, "application/json"), (CONTENT_LENGTH, "1024")],
        Body::from_stream(stream),
    )
        .into_response()
}
