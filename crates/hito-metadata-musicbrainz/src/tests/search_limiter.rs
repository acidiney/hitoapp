use super::{ARTIST_ID, search_protocol::request};
use crate::{Error, MusicBrainzClient};
use axum::{
    Json, Router,
    extract::{Request, State},
    http::StatusCode,
    response::{IntoResponse, Response},
};
use futures_util::FutureExt;
use hito_plugin_sdk::ArtistProfileRequest;
use serde_json::json;
use std::{
    future::poll_fn,
    panic::{AssertUnwindSafe, resume_unwind},
    time::Duration,
};
use tokio::{
    net::TcpListener,
    sync::{mpsc, oneshot},
    task::{JoinHandle, JoinSet},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    Search,
    Profile,
}

type Calls = JoinSet<Result<Kind, Error>>;
type TestResult<T = ()> = Result<T, &'static str>;

struct Driver {
    started: std::time::Instant,
}

impl Driver {
    async fn turn(&self) -> TestResult {
        if self.started.elapsed() >= Duration::from_secs(5) {
            return Err("The owned search pacing workflow exceeded its host-wall deadline");
        }
        tokio::task::yield_now().await;
        Ok(())
    }

    async fn settled(&self) -> TestResult {
        for _ in 0..1024 {
            self.turn().await?;
        }
        Ok(())
    }

    async fn receive<T>(&self, events: &mut mpsc::Receiver<T>) -> TestResult<T> {
        loop {
            match events.try_recv() {
                Ok(event) => return Ok(event),
                Err(mpsc::error::TryRecvError::Empty) => self.turn().await?,
                Err(mpsc::error::TryRecvError::Disconnected) => {
                    return Err("The owned pacing event channel closed");
                }
            }
        }
    }

    async fn completed(&self, calls: &mut Calls) -> TestResult<Kind> {
        loop {
            if let Some(result) = calls.try_join_next() {
                return result
                    .map_err(|_| "The owned metadata task failed")?
                    .map_err(|_| "The owned metadata request failed");
            }
            self.turn().await?;
        }
    }

    async fn quiet(&self, events: &mut mpsc::Receiver<Kind>, calls: &mut Calls) -> TestResult {
        self.settled().await?;
        if !matches!(events.try_recv(), Err(mpsc::error::TryRecvError::Empty)) {
            return Err("A search/profile request reached HTTP before its one-second boundary");
        }
        if calls.try_join_next().is_some() {
            return Err("A waiting metadata call completed before its pacing boundary");
        }
        Ok(())
    }
}

struct Fixture {
    endpoint: reqwest::Url,
    events: mpsc::Receiver<Kind>,
    stop: Option<oneshot::Sender<()>>,
    server: Option<JoinHandle<bool>>,
}

impl Fixture {
    async fn start() -> Self {
        let (events, observed) = mpsc::channel(4);
        let router = Router::new().fallback(respond).with_state(events);
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let endpoint = format!("http://{}/ws/2/", listener.local_addr().unwrap())
            .parse()
            .unwrap();
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
            events: observed,
            stop: Some(stop),
            server: Some(server),
        }
    }

    async fn close(&mut self, driver: &Driver) -> TestResult {
        if let Some(stop) = self.stop.take() {
            let _ = stop.send(());
        }
        let Some(server) = self.server.take() else {
            return Ok(());
        };
        while !server.is_finished() {
            if let Err(error) = driver.turn().await {
                server.abort();
                let _ = server.await;
                return Err(error);
            }
        }
        if !matches!(server.await, Ok(true)) {
            return Err("The owned search pacing fixture failed to stop");
        }
        Ok(())
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

async fn respond(State(events): State<mpsc::Sender<Kind>>, request: Request) -> Response {
    let kind = if request.uri().path() == "/ws/2/artist" {
        Kind::Search
    } else if request.uri().path() == format!("/ws/2/artist/{ARTIST_ID}") {
        Kind::Profile
    } else {
        return StatusCode::NOT_FOUND.into_response();
    };
    if events.try_send(kind).is_err() {
        return StatusCode::TOO_MANY_REQUESTS.into_response();
    }
    match kind {
        Kind::Search => Json(json!({"count": 1, "offset": 0, "artists": [{"id": ARTIST_ID, "name": "Owned pacing ensemble"}]})).into_response(),
        Kind::Profile => Json(json!({"id": ARTIST_ID, "name": "Owned pacing ensemble"})).into_response(),
    }
}

fn start_call(
    client: &MusicBrainzClient,
    calls: &mut Calls,
    kind: Kind,
    entered: Option<mpsc::Sender<()>>,
) {
    let client = client.clone();
    calls.spawn(async move {
        if let Some(entered) = entered {
            let _ = entered.try_send(());
        }
        match kind {
            Kind::Search => client.artist_search(request()).await.map(|_| Kind::Search),
            Kind::Profile => client
                .artist_profile(ArtistProfileRequest {
                    artist_id: ARTIST_ID.into(),
                })
                .await
                .map(|_| Kind::Profile),
        }
    });
}

async fn exercise(
    client: &MusicBrainzClient,
    fixture: &mut Fixture,
    calls: &mut Calls,
    driver: &Driver,
) -> TestResult {
    start_call(client, calls, Kind::Search, None);
    if driver.receive(&mut fixture.events).await? != Kind::Search
        || driver.completed(calls).await? != Kind::Search
    {
        return Err("The first owned search must establish the shared pacing boundary");
    }
    let mut cancelled = Box::pin(client.artist_search(request()));
    if poll_fn(|context| std::task::Poll::Ready(cancelled.as_mut().poll(context)))
        .await
        .is_ready()
    {
        return Err("The cancelled search waiter must first wait for pacing");
    }
    drop(cancelled);
    let (entered, mut entries) = mpsc::channel(2);
    start_call(client, calls, Kind::Profile, Some(entered.clone()));
    start_call(client, calls, Kind::Search, Some(entered));
    driver.receive(&mut entries).await?;
    driver.receive(&mut entries).await?;
    driver.quiet(&mut fixture.events, calls).await?;
    let mut kinds = Vec::new();
    for _ in 0..2 {
        tokio::time::advance(Duration::from_millis(999)).await;
        driver.quiet(&mut fixture.events, calls).await?;
        tokio::time::advance(Duration::from_millis(1)).await;
        let observed = driver.receive(&mut fixture.events).await?;
        if driver.completed(calls).await? != observed {
            return Err("The completed metadata call must match its owned HTTP request");
        }
        kinds.push(observed);
        driver.quiet(&mut fixture.events, calls).await?;
    }
    if kinds.len() != 2
        || !kinds.contains(&Kind::Search)
        || !kinds.contains(&Kind::Profile)
        || !calls.is_empty()
    {
        return Err(
            "Both search and profile clones must share pacing without the cancelled reservation",
        );
    }
    Ok(())
}

#[tokio::test(start_paused = true)]
async fn search_and_profile_clones_share_pacing_without_cancelled_search_reservations() {
    let driver = Driver {
        started: std::time::Instant::now(),
    };
    let mut fixture = Fixture::start().await;
    let client = MusicBrainzClient::with_test_endpoint(fixture.endpoint.clone()).unwrap();
    let mut calls = Calls::new();
    let result = AssertUnwindSafe(exercise(&client, &mut fixture, &mut calls, &driver))
        .catch_unwind()
        .await;
    calls.shutdown().await;
    drop(client);
    let closed = fixture.close(&driver).await;
    let result = match result {
        Ok(result) => result,
        Err(panic) => resume_unwind(panic),
    };
    result.expect("Search and profile operations must share clone pacing");
    closed.expect("The owned search pacing fixture must close and join");
}
