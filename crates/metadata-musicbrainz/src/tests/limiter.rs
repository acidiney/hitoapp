use super::ARTIST_ID;
use crate::{Error, MusicBrainzClient};
use axum::{Json, Router, extract::State};
use hito_plugin_sdk::{ArtistProfileRequest, CatalogArtistProfile};
use serde_json::{Value, json};
use std::{future::poll_fn, time::Duration};
use tokio::{
    net::TcpListener,
    sync::{mpsc, oneshot},
    task::{JoinHandle, JoinSet},
};

type Calls = JoinSet<Result<CatalogArtistProfile, Error>>;
type TestResult<T = ()> = Result<T, &'static str>;

struct Driver {
    started: std::time::Instant,
}

impl Driver {
    async fn turn(&self) -> TestResult {
        if self.started.elapsed() >= Duration::from_secs(5) {
            return Err("the owned metadata workflow exceeded its host-wall deadline");
        }
        tokio::task::yield_now().await;
        Ok(())
    }

    async fn settled(&self) -> TestResult {
        for _ in 0..1_024 {
            self.turn().await?;
        }
        Ok(())
    }

    async fn receive(&self, events: &mut mpsc::UnboundedReceiver<()>) -> TestResult {
        loop {
            match events.try_recv() {
                Ok(()) => return Ok(()),
                Err(mpsc::error::TryRecvError::Empty) => self.turn().await?,
                Err(mpsc::error::TryRecvError::Disconnected) => {
                    return Err("the owned metadata event channel closed");
                }
            }
        }
    }

    async fn completed(&self, calls: &mut Calls) -> TestResult {
        loop {
            if let Some(result) = calls.try_join_next() {
                return result
                    .map_err(|_| "the owned metadata call task failed")?
                    .map(|_| ())
                    .map_err(|_| "the owned metadata profile call failed");
            }
            self.turn().await?;
        }
    }

    async fn quiet(
        &self,
        events: &mut mpsc::UnboundedReceiver<()>,
        calls: &mut Calls,
    ) -> TestResult {
        self.settled().await?;
        if !matches!(events.try_recv(), Err(mpsc::error::TryRecvError::Empty)) {
            return Err("a MusicBrainz request started before its one-second boundary");
        }
        if calls.try_join_next().is_some() {
            return Err("a waiting MusicBrainz call completed before its boundary");
        }
        Ok(())
    }
}

struct Fixture {
    endpoint: reqwest::Url,
    events: mpsc::UnboundedReceiver<()>,
    stop: Option<oneshot::Sender<()>>,
    server: Option<JoinHandle<()>>,
}

impl Fixture {
    async fn start() -> Self {
        let (events, observed) = mpsc::unbounded_channel();
        let router = Router::new().fallback(profile).with_state(events);
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("the owned limiter fixture must bind");
        let endpoint = format!(
            "http://{}/ws/2/",
            listener
                .local_addr()
                .expect("the fixture must have an address")
        )
        .parse()
        .expect("the owned limiter endpoint must be valid");
        let (stop, stopped) = oneshot::channel();
        let server = tokio::spawn(async move {
            axum::serve(listener, router)
                .with_graceful_shutdown(async move {
                    let _ = stopped.await;
                })
                .await
                .expect("the owned limiter fixture must run");
        });
        Self {
            endpoint,
            events: observed,
            stop: Some(stop),
            server: Some(server),
        }
    }

    async fn close(mut self, driver: &Driver) -> TestResult {
        if let Some(stop) = self.stop.take() {
            let _ = stop.send(());
        }
        if let Some(server) = self.server.take() {
            while !server.is_finished() {
                if let Err(error) = driver.turn().await {
                    server.abort();
                    let _ = server.await;
                    return Err(error);
                }
            }
            server
                .await
                .map_err(|_| "the owned limiter fixture failed to stop")?;
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

async fn profile(State(events): State<mpsc::UnboundedSender<()>>) -> Json<Value> {
    let _ = events.send(());
    Json(json!({"id": ARTIST_ID, "name": "Owned limiter ensemble"}))
}

fn start_call(
    client: &MusicBrainzClient,
    calls: &mut Calls,
    entered: Option<mpsc::UnboundedSender<()>>,
) {
    let client = client.clone();
    calls.spawn(async move {
        if let Some(entered) = entered {
            let _ = entered.send(());
        }
        client.artist_profile(request()).await
    });
}

fn request() -> ArtistProfileRequest {
    ArtistProfileRequest {
        artist_id: ARTIST_ID.into(),
    }
}

async fn exercise(
    client: &MusicBrainzClient,
    fixture: &mut Fixture,
    calls: &mut Calls,
    driver: &Driver,
) -> TestResult {
    start_call(client, calls, None);
    driver.receive(&mut fixture.events).await?;
    driver.completed(calls).await?;

    let mut cancelled = Box::pin(client.artist_profile(request()));
    if poll_fn(|context| std::task::Poll::Ready(cancelled.as_mut().poll(context)))
        .await
        .is_ready()
    {
        return Err("the waiting MusicBrainz call completed before cancellation");
    }
    drop(cancelled);

    let (entered, mut entries) = mpsc::unbounded_channel();
    start_call(client, calls, Some(entered.clone()));
    start_call(client, calls, Some(entered));
    driver.receive(&mut entries).await?;
    driver.receive(&mut entries).await?;
    driver.quiet(&mut fixture.events, calls).await?;

    tokio::time::advance(Duration::from_millis(999)).await;
    driver.quiet(&mut fixture.events, calls).await?;
    tokio::time::advance(Duration::from_millis(1)).await;
    driver.receive(&mut fixture.events).await?;
    driver.completed(calls).await?;
    driver.quiet(&mut fixture.events, calls).await?;

    tokio::time::advance(Duration::from_millis(999)).await;
    driver.quiet(&mut fixture.events, calls).await?;
    tokio::time::advance(Duration::from_millis(1)).await;
    driver.receive(&mut fixture.events).await?;
    driver.completed(calls).await?;
    if !calls.is_empty() || fixture.events.try_recv().is_ok() {
        return Err("the cancelled MusicBrainz waiter reached the provider");
    }
    Ok(())
}

#[tokio::test(start_paused = true)]
async fn client_clones_share_one_second_start_spacing_without_cancelled_reservations() {
    let driver = Driver {
        started: std::time::Instant::now(),
    };
    let mut fixture = Fixture::start().await;
    let client = MusicBrainzClient::with_test_endpoint(fixture.endpoint.clone())
        .expect("the owned limiter client must initialize");
    let mut calls = Calls::new();

    let outcome = exercise(&client, &mut fixture, &mut calls, &driver).await;
    calls.shutdown().await;
    drop(client);
    let closed = fixture.close(&driver).await;

    outcome.expect("cloned MusicBrainz clients must share cancellation-safe start spacing");
    closed.expect("the owned limiter fixture must close and join");
}
