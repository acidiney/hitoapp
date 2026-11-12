use super::setup::Prepared;
use axum::{Router, http::StatusCode};
use hito_core::{AutomaticLyricsLookup, Services};
use serde_json::json;
use std::{
    net::SocketAddr,
    time::{Duration, Instant},
};
use tokio::{
    io::AsyncWriteExt,
    net::{TcpListener, TcpStream},
    sync::oneshot,
    task::JoinHandle,
};

pub(super) struct Runtime {
    pub(super) address: SocketAddr,
    stop: Option<oneshot::Sender<()>>,
    task: Option<JoinHandle<()>>,
}
impl Runtime {
    pub(super) async fn start(app: Router, services: Services) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let (stop, stopped) = oneshot::channel();
        let task = tokio::spawn(async move {
            hito_server::serve(listener, app, services, async {
                let _ = stopped.await;
            })
            .await
            .unwrap();
        });
        Self {
            address,
            stop: Some(stop),
            task: Some(task),
        }
    }
    pub(super) async fn finish(mut self) {
        let _ = self.stop.take().unwrap().send(());
        tokio::time::timeout(Duration::from_secs(8), self.task.take().unwrap())
            .await
            .unwrap()
            .unwrap();
    }
}
impl Drop for Runtime {
    fn drop(&mut self) {
        if let Some(stop) = self.stop.take() {
            let _ = stop.send(());
        }
        if let Some(task) = self.task.take() {
            task.abort();
        }
    }
}

#[tokio::test]
async fn explicit_cancellation_and_real_tcp_eof_retire_provider_bodies_and_release_the_same_account_quota()
 {
    let prepared = Prepared::new().await;
    let gate = prepared.provider.hold("search");
    let catalog = prepared.library.services.catalog.clone();
    let actor = prepared.actor().await;
    let recording = prepared.library.recording.clone();
    let (cancel, cancelled) = oneshot::channel();
    let task = tokio::spawn(async move {
        catalog
            .automatic_recording_lyrics_until(&actor, &recording, async {
                let _ = cancelled.await;
            })
            .await
    });
    tokio::time::timeout(Duration::from_secs(3), gate.entered())
        .await
        .unwrap();
    let (status, _) = prepared
        .library
        .account
        .request(&prepared.library.app, "POST", &prepared.path(), json!({}))
        .await;
    assert_eq!(status, StatusCode::CONFLICT);
    cancel.send(()).unwrap();
    assert!(matches!(
        tokio::time::timeout(Duration::from_secs(2), task)
            .await
            .unwrap()
            .unwrap(),
        Ok(AutomaticLyricsLookup::Cancelled)
    ));
    tokio::time::timeout(Duration::from_secs(2), gate.retired())
        .await
        .unwrap();
    assert!(!gate.released());
    prepared.provider.release();
    assert_eq!(prepared.read().await["status"], "ready_v2");

    let runtime = Runtime::start(
        prepared.library.app.clone(),
        prepared.library.services.clone(),
    )
    .await;
    let gate = prepared.provider.hold("metadata");
    let mut socket = TcpStream::connect(runtime.address).await.unwrap();
    let mut request = format!(
        "POST {} HTTP/1.1\r\nHost: localhost:8080\r\nConnection: keep-alive\r\nContent-Length: 2\r\n",
        prepared.path()
    );
    for (name, value) in prepared.library.account.headers() {
        request.push_str(name.unwrap().as_str());
        request.push_str(": ");
        request.push_str(value.to_str().unwrap());
        request.push_str("\r\n");
    }
    request.push_str("\r\n{}");
    socket.write_all(request.as_bytes()).await.unwrap();
    tokio::time::timeout(Duration::from_secs(3), gate.entered())
        .await
        .unwrap();
    drop(socket);
    tokio::time::timeout(Duration::from_secs(2), gate.retired())
        .await
        .expect("Actual client EOF must cancel the handler and retire its provider body");
    assert!(!gate.released());
    prepared.provider.release();
    assert_eq!(prepared.read().await["status"], "ready_v2");
    assert_eq!(
        prepared
            .library
            .account
            .get(
                &prepared.library.app,
                &prepared.path().replace("/automatic", "/identities")
            )
            .await["items"],
        json!([])
    );
    prepared.library.assert_original();
    runtime.finish().await;
    prepared.finish().await;
}

#[tokio::test]
async fn search_metadata_and_negotiation_share_one_original_ten_second_deadline() {
    let prepared = Prepared::new().await;
    let search = prepared.provider.hold("search");
    let catalog = prepared.library.services.catalog.clone();
    let actor = prepared.actor().await;
    let recording = prepared.library.recording.clone();
    let started = Instant::now();
    let mut task = tokio::spawn(async move {
        catalog
            .automatic_recording_lyrics_until(&actor, &recording, std::future::pending())
            .await
    });
    tokio::time::timeout(Duration::from_secs(2), search.entered())
        .await
        .unwrap();
    tokio::time::sleep(Duration::from_secs(4)).await;
    prepared.provider.release();
    let metadata = prepared.provider.hold("metadata");
    tokio::time::timeout(Duration::from_secs(2), metadata.entered())
        .await
        .unwrap();
    tokio::time::sleep(Duration::from_secs(4)).await;
    prepared.provider.release();
    let descriptor = prepared.provider.hold("v2_descriptor");
    tokio::time::timeout(Duration::from_secs(2), descriptor.entered())
        .await
        .unwrap();
    let completed = tokio::time::timeout(Duration::from_secs(3), &mut task).await;
    if completed.is_err() {
        task.abort();
        let _ = task.await;
    }
    let response = completed
        .expect("Each network hop must retain the original deadline")
        .unwrap();
    assert!(matches!(response, Ok(AutomaticLyricsLookup::Unavailable)));
    assert!(
        started.elapsed() >= Duration::from_millis(9500)
            && started.elapsed() < Duration::from_secs(12)
    );
    tokio::time::timeout(Duration::from_secs(2), descriptor.retired())
        .await
        .unwrap();
    assert!(!descriptor.released());
    prepared.provider.release();
    assert_eq!(prepared.read().await["status"], "ready_v2");
    prepared.finish().await;
}
