use super::{
    authority::{Change, change, set_pin},
    cancellation::Runtime,
    setup::Prepared,
    timed_priority::{candidate, content},
};
use crate::account;
use hito_core::AutomaticLyricsLookup;
use serde_json::json;
use std::time::{Duration, Instant};
use tokio::{io::AsyncWriteExt, net::TcpStream};

fn pair(prepared: &Prepared) {
    let plain = candidate(&prepared.provider, "owned:plain", Some(content(false)));
    let timed = candidate(&prepared.provider, "owned:later-timed", Some(content(true)));
    prepared
        .provider
        .reply("search", json!({"items":[plain,timed],"next_cursor":null}));
}

#[tokio::test]
async fn a_held_later_candidate_rechecks_authority_before_a_plain_fallback_can_escape() {
    for change_kind in [
        Change::GrantEpoch,
        Change::MetadataEpoch,
        Change::Installation,
        Change::Service,
        Change::Session,
        Change::PinSetup,
        Change::PinPolicy,
        Change::PinLock,
    ] {
        let prepared = Prepared::new().await;
        if matches!(change_kind, Change::PinPolicy | Change::PinLock) {
            set_pin(&prepared).await;
        }
        pair(&prepared);
        let first = prepared.provider.hold("v2_lyrics");
        let app = prepared.library.app.clone();
        let path = prepared.path();
        let headers = prepared.library.account.headers();
        let mut task = tokio::spawn(async move {
            account::exchange(&app, "POST", &path, json!({}), headers).await
        });
        tokio::time::timeout(Duration::from_secs(2), first.entered())
            .await
            .unwrap();
        let later = prepared.provider.advance_hold("metadata");
        tokio::time::timeout(Duration::from_secs(2), later.entered())
            .await
            .unwrap();
        assert_eq!(prepared.provider.requests_for("v2_lyrics").len(), 1);
        let expected = change(&prepared, change_kind).await;
        let completed = tokio::time::timeout(Duration::from_secs(2), &mut task).await;
        let rejected = matches!(&completed, Ok(Ok((status, _, value))) if expected.contains(status) && value.get("document").is_none());
        if completed.is_err() {
            task.abort();
            let _ = task.await;
        }
        tokio::time::timeout(Duration::from_secs(2), later.retired())
            .await
            .unwrap();
        let unreleased = !later.released();
        let documents = prepared.provider.requests_for("v2_lyrics").len();
        prepared.finish().await;
        assert!(
            rejected && unreleased,
            "Changed authority must retire the held later candidate without publishing either document"
        );
        assert_eq!(documents, 1);
    }
}

#[tokio::test]
async fn real_client_eof_retires_a_later_candidate_and_releases_the_plain_fallback_admission() {
    let prepared = Prepared::new().await;
    pair(&prepared);
    let before = prepared.library.snapshot().await;
    let runtime = Runtime::start(
        prepared.library.app.clone(),
        prepared.library.services.clone(),
    )
    .await;
    let first = prepared.provider.hold("v2_lyrics");
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
    tokio::time::timeout(Duration::from_secs(2), first.entered())
        .await
        .unwrap();
    let later = prepared.provider.advance_hold("metadata");
    tokio::time::timeout(Duration::from_secs(2), later.entered())
        .await
        .unwrap();
    drop(socket);
    tokio::time::timeout(Duration::from_secs(2), later.retired())
        .await
        .expect("Client EOF must retire the later provider body before releasing its fixture gate");
    assert!(!later.released());
    prepared.provider.release();
    assert_eq!(
        prepared.read().await["provider_recording"]["id"],
        "owned:later-timed"
    );
    assert_eq!(prepared.library.snapshot().await, before);
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
    runtime.finish().await;
    prepared.finish().await;
}

#[tokio::test]
async fn later_candidate_reads_keep_the_original_ten_second_deadline_and_discard_plain_on_expiry() {
    let prepared = Prepared::new().await;
    pair(&prepared);
    let before = prepared.library.snapshot().await;
    let first = prepared.provider.hold("v2_lyrics");
    let catalog = prepared.library.services.catalog.clone();
    let actor = prepared.actor().await;
    let recording = prepared.library.recording.clone();
    let started = Instant::now();
    let mut task = tokio::spawn(async move {
        catalog
            .automatic_recording_lyrics_until(&actor, &recording, std::future::pending())
            .await
    });
    tokio::time::timeout(Duration::from_secs(2), first.entered())
        .await
        .unwrap();
    tokio::time::sleep(Duration::from_secs(4)).await;
    let metadata = prepared.provider.advance_hold("metadata");
    tokio::time::timeout(Duration::from_secs(2), metadata.entered())
        .await
        .unwrap();
    tokio::time::sleep(Duration::from_secs(4)).await;
    let later = prepared.provider.advance_hold("v2_lyrics");
    tokio::time::timeout(Duration::from_secs(2), later.entered())
        .await
        .unwrap();
    let completed = tokio::time::timeout(Duration::from_secs(3), &mut task).await;
    if completed.is_err() {
        task.abort();
        let _ = task.await;
    }
    let response = completed
        .expect("A later candidate must not renew the original lookup budget")
        .unwrap();
    let elapsed = started.elapsed();
    tokio::time::timeout(Duration::from_secs(2), later.retired())
        .await
        .unwrap();
    let unreleased = !later.released();
    prepared.provider.release();
    assert_eq!(prepared.library.snapshot().await, before);
    prepared.finish().await;
    assert!(matches!(response, Ok(AutomaticLyricsLookup::Unavailable)));
    assert!(unreleased);
    assert!(elapsed >= Duration::from_millis(9500) && elapsed < Duration::from_secs(12));
}
