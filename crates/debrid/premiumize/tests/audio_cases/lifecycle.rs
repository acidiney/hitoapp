use super::super::fixture::response::Witness;
use super::*;
use std::time::{Duration, Instant};
use tokio::sync::oneshot;

async fn witness(observed: oneshot::Receiver<()>) {
    tokio::time::timeout(Duration::from_secs(2), observed)
        .await
        .expect("the actual owned TLS witness exceeded its deadline")
        .expect("the actual owned TLS witness disappeared");
}

#[tokio::test]
async fn aggregate_deadline_covers_api_headers_item_body_and_media_headers_without_retry() {
    for stage in 0..3 {
        let mut api = Vec::new();
        let mut media = Vec::new();
        let (held, observed) = match stage {
            0 => json(&transfers()).held(None),
            1 => json(&item()).held(Some(1)),
            _ => Response::wave(owned_wave()).held(None),
        };
        if stage > 0 {
            api.push(json(&transfers()));
        }
        if stage == 2 {
            api.push(json(&item()));
            media.push(held);
        } else {
            api.push(held);
        }
        api.extend([json(&transfers()), json(&item())]);
        media.push(Response::wave(owned_wave()));
        let journey = Journey::start(api, media).await;
        let retained = journey.client.clone();
        let capture = Capture::default();
        let client = journey.client.clone();
        let dispatch = capture.dispatch();
        let configured = binding();
        let allowlist = MediaAllowlist::new(vec![MEDIA_ORIGIN.into()]).unwrap();
        let started = Instant::now();
        let task = tokio::spawn(async move {
            client
                .open_bound_audio(&configured, &allowlist)
                .with_subscriber(dispatch)
                .await
        });
        let Witness {
            reached,
            peer_closed,
            release,
        } = observed;
        witness(reached).await;
        let result = tokio::time::timeout(Duration::from_secs(11), task)
            .await
            .unwrap()
            .unwrap();
        rejected(result, Error::Deadline);
        assert!(started.elapsed() >= Duration::from_millis(8700));
        assert!(started.elapsed() < Duration::from_millis(9750));
        witness(peer_closed).await;
        drop(release);
        assert_eq!(journey.api.request_count(), if stage == 0 { 1 } else { 2 });
        assert_eq!(journey.media.request_count(), usize::from(stage == 2));
        async {
            let body = retained
                .open_bound_audio(&binding(), &journey.allowlist)
                .await
                .unwrap();
            collect(body, &binding()).await;
        }
        .with_subscriber(capture.dispatch())
        .await;
        journey
            .close(
                if stage == 0 { 3 } else { 4 },
                if stage == 2 { 2 } else { 1 },
            )
            .await;
        logs(
            &capture,
            &[
                Expected::Opening("deadline"),
                Expected::Opening("complete"),
                Expected::Body("complete", binding().file_bytes),
            ],
        );
        drop(retained);
    }
}

#[tokio::test]
async fn cancelled_item_body_closes_real_peer_while_a_client_clone_survives_and_recovers() {
    let (held, observed) = json(&item()).held(Some(1));
    let journey = Journey::start(
        vec![json(&transfers()), held, json(&transfers()), json(&item())],
        vec![Response::wave(owned_wave())],
    )
    .await;
    let retained = journey.client.clone();
    let capture = Capture::default();
    let decoy = Capture::default();
    let client = journey.client.clone();
    let configured = binding();
    let allowlist = MediaAllowlist::new(vec![MEDIA_ORIGIN.into()]).unwrap();
    let dispatch = capture.dispatch();
    let task = tokio::spawn(async move {
        client
            .open_bound_audio(&configured, &allowlist)
            .with_subscriber(dispatch)
            .await
    });
    let Witness {
        reached,
        peer_closed,
        release,
    } = observed;
    witness(reached).await;
    tracing::dispatcher::with_default(&decoy.dispatch(), || task.abort());
    match task.await {
        Err(error) => assert!(error.is_cancelled()),
        Ok(_) => panic!("the held opening was not cancelled"),
    }
    witness(peer_closed).await;
    drop(release);
    assert_eq!(journey.api.request_count(), 2);
    assert_eq!(journey.media.request_count(), 0);
    async {
        let body = retained
            .open_bound_audio(&binding(), &journey.allowlist)
            .await
            .unwrap();
        collect(body, &binding()).await;
    }
    .with_subscriber(capture.dispatch())
    .await;
    journey.close(4, 1).await;
    logs(
        &capture,
        &[
            Expected::Opening("cancelled"),
            Expected::Opening("complete"),
            Expected::Body("complete", binding().file_bytes),
        ],
    );
    decoy.assert_empty();
    drop(retained);
}

#[tokio::test]
async fn two_retained_bodies_hold_admission_and_drop_closes_peers_before_recovery() {
    let (first, first_observed) = Response::wave(owned_wave()).held(Some(44));
    let (second, second_observed) = Response::wave(owned_wave()).held(Some(44));
    let journey = Journey::start(
        (0..3)
            .flat_map(|_| [json(&transfers()), json(&item())])
            .collect(),
        vec![first, second, Response::wave(owned_wave())],
    )
    .await;
    let retained = journey.client.clone();
    let capture = Capture::default();
    let decoy = Capture::default();
    let first = retained
        .open_bound_audio(&binding(), &journey.allowlist)
        .with_subscriber(capture.dispatch())
        .await
        .unwrap();
    let second = retained
        .open_bound_audio(&binding(), &journey.allowlist)
        .with_subscriber(capture.dispatch())
        .await
        .unwrap();
    let Witness {
        reached: first_reached,
        peer_closed: first_closed,
        release: first_release,
    } = first_observed;
    let Witness {
        reached: second_reached,
        peer_closed: second_closed,
        release: second_release,
    } = second_observed;
    witness(first_reached).await;
    witness(second_reached).await;
    assert_eq!(journey.api.request_count(), 4);
    assert_eq!(journey.media.request_count(), 2);
    rejected(
        retained
            .open_bound_audio(&binding(), &journey.allowlist)
            .with_subscriber(capture.dispatch())
            .await,
        Error::Busy,
    );
    assert_eq!(journey.api.request_count(), 4);
    assert_eq!(journey.media.request_count(), 2);
    tracing::dispatcher::with_default(&decoy.dispatch(), || drop(first));
    witness(first_closed).await;
    drop(first_release);
    async {
        let body = retained
            .open_bound_audio(&binding(), &journey.allowlist)
            .await
            .unwrap();
        collect(body, &binding()).await;
    }
    .with_subscriber(capture.dispatch())
    .await;
    tracing::dispatcher::with_default(&decoy.dispatch(), || drop(second));
    witness(second_closed).await;
    drop(second_release);
    journey.close(6, 3).await;
    logs(
        &capture,
        &[
            Expected::Opening("complete"),
            Expected::Opening("complete"),
            Expected::Opening("busy"),
            Expected::Body("cancelled", 0),
            Expected::Opening("complete"),
            Expected::Body("complete", binding().file_bytes),
            Expected::Body("cancelled", 0),
        ],
    );
    decoy.assert_empty();
    drop(retained);
}
