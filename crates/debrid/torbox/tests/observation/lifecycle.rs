use super::*;
use std::{
    future::{Future, poll_fn},
    task::Poll,
    time::Duration,
};
use support::reply::Reply;

#[tokio::test]
async fn clones_share_two_bounded_slots_and_cancelled_observation_releases_admission() {
    let (held, received, release) = Reply::paused(observation(true, true, "album/owned.wav"));
    let fixture = Fixture::responses(vec![
        held,
        Reply::json(observation(true, true, "album/owned.wav")),
    ])
    .await;
    let client =
        TorBoxClient::with_test_tls(TOKEN.into(), fixture.ca_der(), fixture.address()).unwrap();
    let capture = Capture::default();
    let first_client = client.clone();
    let first_capture = capture.clone();
    let first = tokio::spawn(async move {
        first_client
            .observe(&configured_binding())
            .with_subscriber(first_capture.dispatch())
            .await
    });
    tokio::time::timeout(Duration::from_secs(2), received)
        .await
        .unwrap()
        .unwrap();
    let binding = configured_binding();
    let mut second = Box::pin(client.observe(&binding).with_subscriber(capture.dispatch()));
    assert!(matches!(
        poll_fn(|cx| Poll::Ready(second.as_mut().poll(cx))).await,
        Poll::Pending
    ));
    let busy = tokio::time::timeout(
        Duration::from_millis(100),
        client.observe(&binding).with_subscriber(capture.dispatch()),
    )
    .await;
    assert!(
        matches!(busy, Ok(Err(Error::Busy))),
        "admission must reject excess observations without waiting or dispatching"
    );
    drop(second);
    let mut replacement = Box::pin(client.observe(&binding).with_subscriber(capture.dispatch()));
    assert!(matches!(
        poll_fn(|cx| Poll::Ready(replacement.as_mut().poll(cx))).await,
        Poll::Pending
    ));
    let busy = tokio::time::timeout(
        Duration::from_millis(100),
        client.observe(&binding).with_subscriber(capture.dispatch()),
    )
    .await;
    assert!(matches!(busy, Ok(Err(Error::Busy))));
    drop(replacement);
    release.send(()).unwrap();
    assert!(first.await.unwrap().is_ok());
    assert!(
        client
            .observe(&binding)
            .with_subscriber(capture.dispatch())
            .await
            .is_ok()
    );
    assert_eq!(fixture.close().await.len(), 2);
    capture.assert_observations(
        &[
            ("busy", 0),
            ("cancelled", 0),
            ("busy", 0),
            ("cancelled", 0),
            ("provider_completed", 1),
            ("provider_completed", 1),
        ],
        &[TOKEN, "album/owned.wav"],
    );
}

#[tokio::test]
async fn clones_pace_consecutive_observations_without_retries() {
    let fixture = Fixture::start(vec![observation(true, true, "album/owned.wav"); 2]).await;
    let client =
        TorBoxClient::with_test_tls(TOKEN.into(), fixture.ca_der(), fixture.address()).unwrap();
    assert!(client.observe(&configured_binding()).await.is_ok());
    assert!(client.clone().observe(&configured_binding()).await.is_ok());
    let requests = fixture.close().await;
    assert_eq!(requests.len(), 2);
    assert!(
        requests[1]
            .received_at
            .duration_since(requests[0].received_at)
            >= Duration::from_millis(200),
        "a shared client must pace API requests below the documented 300 per-minute ceiling"
    );
}

#[tokio::test]
async fn aborting_dispatched_observation_emits_one_cancelled_outcome_and_stops_owned_io() {
    let (mut held, received, _release) = Reply::paused(observation(true, true, "album/owned.wav"));
    let peer_closed = held.observe_peer_close();
    let fixture = Fixture::responses(vec![held]).await;
    let client =
        TorBoxClient::with_test_tls(TOKEN.into(), fixture.ca_der(), fixture.address()).unwrap();
    let capture = Capture::default();
    let dispatch = capture.dispatch();
    let observing = client.clone();
    let task = tokio::spawn(async move {
        observing
            .observe(&configured_binding())
            .with_subscriber(dispatch)
            .await
    });
    tokio::time::timeout(Duration::from_secs(2), received)
        .await
        .unwrap()
        .unwrap();
    task.abort();
    assert!(matches!(task.await, Err(error) if error.is_cancelled()));
    tokio::time::timeout(Duration::from_secs(2), peer_closed)
        .await
        .expect("aborting the request must close its owned peer before fixture shutdown")
        .expect("the held TLS handler must witness peer closure");
    drop(client);
    assert_eq!(fixture.close().await.len(), 1);
    capture.assert_observations(&[("cancelled", 0)], &[TOKEN, "album/owned.wav"]);
}

#[tokio::test]
async fn unresponsive_api_hits_its_deadline_once_and_releases_admission_for_recovery() {
    let (held, received, release) = Reply::paused(observation(true, true, "album/owned.wav"));
    let fixture = Fixture::responses(vec![
        held,
        Reply::json(observation(true, true, "album/owned.wav")),
    ])
    .await;
    let client =
        TorBoxClient::with_test_tls(TOKEN.into(), fixture.ca_der(), fixture.address()).unwrap();
    let capture = Capture::default();
    let stalled_client = client.clone();
    let dispatch = capture.dispatch();
    let stalled = tokio::spawn(async move {
        stalled_client
            .observe(&configured_binding())
            .with_subscriber(dispatch)
            .await
    });
    tokio::time::timeout(Duration::from_secs(2), received)
        .await
        .unwrap()
        .unwrap();
    let result = tokio::time::timeout(Duration::from_secs(12), stalled)
        .await
        .unwrap()
        .unwrap();
    assert!(matches!(result, Err(Error::Deadline)));
    release.send(()).unwrap();
    assert!(
        client
            .observe(&configured_binding())
            .with_subscriber(capture.dispatch())
            .await
            .is_ok()
    );
    assert_eq!(fixture.close().await.len(), 2);
    capture.assert_observations(
        &[("deadline", 0), ("provider_completed", 1)],
        &[TOKEN, "album/owned.wav"],
    );
}
