use super::super::wire_fixture::response::Witness;
use super::*;
use std::time::{Duration, Instant};
use tokio::sync::oneshot;

async fn witnessed(observed: oneshot::Receiver<()>) {
    tokio::time::timeout(Duration::from_secs(2), observed)
        .await
        .expect("the real owned TLS witness exceeded its deadline")
        .expect("the real owned TLS witness disappeared");
}

#[tokio::test]
async fn cloned_api_admission_is_bounded_and_cancelled_item_bodies_close_before_manual_recovery() {
    let (first_item, first_witness) = json(&item()).held(Some(1));
    let (second_item, second_witness) = json(&item()).held(Some(1));
    let journey = Journey::start(vec![
        json(&transfers()),
        first_item,
        json(&transfers()),
        second_item,
        json(&transfers()),
        json(&item()),
    ])
    .await;
    let retained = journey.client.clone();
    let first_client = retained.clone();
    let second_client = retained.clone();
    let configured = binding();
    let capture = Capture::default();
    let decoy = Capture::default();
    let Witness {
        reached: first_reached,
        peer_closed: first_closed,
        release: first_release,
    } = first_witness;
    let Witness {
        reached: second_reached,
        peer_closed: second_closed,
        release: second_release,
    } = second_witness;
    let mut first = Box::pin(first_client.observe_bound_file(&configured));
    async {
        tokio::select! {
            _ = &mut first => panic!("the first held item body completed before release"),
            _ = witnessed(first_reached) => {},
        }
    }
    .with_subscriber(capture.dispatch())
    .await;
    let mut second = Box::pin(second_client.observe_bound_file(&configured));
    async {
        tokio::select! {
            _ = &mut second => panic!("the second held item body completed before release"),
            _ = witnessed(second_reached) => {},
        }
    }
    .with_subscriber(capture.dispatch())
    .await;
    assert_eq!(journey.api.request_count(), 4);
    assert_eq!(journey.media.request_count(), 0);
    rejected(
        retained
            .observe_bound_file(&configured)
            .with_subscriber(capture.dispatch())
            .await,
        Error::Busy,
    );
    assert_eq!(journey.api.request_count(), 4);
    assert_eq!(journey.media.request_count(), 0);
    tracing::dispatcher::with_default(&decoy.dispatch(), || drop(first));
    witnessed(first_closed).await;
    drop(first_release);
    available(
        retained
            .observe_bound_file(&configured)
            .with_subscriber(capture.dispatch())
            .await
            .unwrap(),
    );
    assert_eq!(journey.api.request_count(), 6);
    assert_eq!(journey.media.request_count(), 0);
    second_release
        .send(())
        .expect("the second owned item body must remain held");
    available(second.with_subscriber(capture.dispatch()).await.unwrap());
    drop(second_closed);
    journey
        .close(
            &[
                ApiCall::Transfers,
                ApiCall::Item,
                ApiCall::Transfers,
                ApiCall::Item,
                ApiCall::Transfers,
                ApiCall::Item,
            ],
            0,
        )
        .await;
    logs(
        &capture,
        &[
            "busy",
            "cancelled",
            "provider_completed",
            "provider_completed",
        ],
    );
    decoy.assert_empty();
    drop(retained);
}

#[tokio::test]
async fn nine_second_aggregate_deadline_retires_api_headers_and_item_bodies_without_retry() {
    for at_item in [false, true] {
        let (held, witness) = if at_item {
            json(&item()).held(Some(1))
        } else {
            json(&transfers()).held(None)
        };
        let mut replies = if at_item {
            vec![json(&transfers())]
        } else {
            Vec::new()
        };
        replies.push(held);
        replies.extend([json(&transfers()), json(&item())]);
        let journey = Journey::start(replies).await;
        let retained = journey.client.clone();
        let client = retained.clone();
        let capture = Capture::default();
        let dispatch = capture.dispatch();
        let started = Instant::now();
        let task = tokio::spawn(async move {
            client
                .observe_bound_file(&binding())
                .with_subscriber(dispatch)
                .await
        });
        let Witness {
            reached,
            peer_closed,
            release,
        } = witness;
        witnessed(reached).await;
        let result = tokio::time::timeout(Duration::from_secs(11), task)
            .await
            .unwrap()
            .unwrap();
        rejected(result, Error::Deadline);
        let elapsed = started.elapsed();
        assert!(elapsed >= Duration::from_millis(8700));
        assert!(elapsed < Duration::from_millis(9750));
        witnessed(peer_closed).await;
        drop(release);
        assert_eq!(journey.api.request_count(), if at_item { 2 } else { 1 });
        assert_eq!(journey.media.request_count(), 0);
        available(
            retained
                .observe_bound_file(&binding())
                .with_subscriber(capture.dispatch())
                .await
                .unwrap(),
        );
        let mut calls = if at_item {
            vec![ApiCall::Transfers]
        } else {
            Vec::new()
        };
        calls.extend([
            if at_item {
                ApiCall::Item
            } else {
                ApiCall::Transfers
            },
            ApiCall::Transfers,
            ApiCall::Item,
        ]);
        journey.close(&calls, 0).await;
        logs(&capture, &["deadline", "provider_completed"]);
        drop(retained);
    }
}
