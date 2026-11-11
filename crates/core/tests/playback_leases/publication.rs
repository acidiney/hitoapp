use super::work::{fixture::Case, poll_once};
use crate::{AudioRequest, OpenedAudio};
use futures_util::FutureExt;
use std::{future::Future, pin::Pin, time::Duration};
use tokio::sync::Notify;

#[tokio::test]
async fn completed_read_replies_cannot_publish_bytes_after_observed_retirement() {
    for retirement in ["close", "expiry", "membership"] {
        let f = Case::new().await;
        if retirement == "expiry" {
            f.services.playback.leases.state.lock().unwrap().lifetime =
                Some(Duration::from_secs(2));
        }
        let lease = f
            .services
            .playback
            .begin_lease(&f.actor, f.request())
            .await
            .unwrap();
        let entry = f
            .services
            .playback
            .leases
            .entry(&f.actor, lease.id)
            .unwrap();
        let OpenedAudio::Content(mut audio) = f
            .services
            .playback
            .open_lease_audio(&f.actor, lease.id, AudioRequest::default())
            .await
            .unwrap()
        else {
            panic!("body")
        };
        let mut body = audio.body.take().unwrap();
        let mut reading = Box::pin(body.chunk());
        assert!(poll_once(reading.as_mut()).await.is_pending());
        entry.publications.read.notified().await;
        assert_eq!(
            entry.bytes_read.load(std::sync::atomic::Ordering::Relaxed),
            f.original.len() as u64,
            "the owner actually queued a complete successful chunk"
        );
        match retirement {
            "close" => f.services.playback.close().await.unwrap(),
            "expiry" => {
                tokio::time::sleep(Duration::from_millis(2100)).await;
                f.services
                    .playback
                    .release_lease(&f.actor, lease.id)
                    .await
                    .unwrap();
            }
            "membership" => {
                f.services
                    .identity
                    .set_library_access(&f.actor, &f.library, &f.actor.user().id, false)
                    .await
                    .unwrap();
                f.services
                    .identity
                    .set_library_access(&f.actor, &f.library, &f.actor.user().id, true)
                    .await
                    .unwrap();
                assert!(
                    f.services
                        .playback
                        .lease_status(&f.actor, lease.id)
                        .await
                        .is_err()
                );
                f.services
                    .playback
                    .release_lease(&f.actor, lease.id)
                    .await
                    .unwrap();
            }
            _ => unreachable!(),
        }
        drop(entry);
        f.close().await;
        assert!(
            reading.await.is_err(),
            "a queued chunk cannot publish after {retirement} joins its owner"
        );
    }
}

#[tokio::test(flavor = "current_thread")]
async fn creation_and_open_replies_recheck_authority_after_the_delivery_handoff() {
    let mut rejected = Vec::new();
    for stage in ["creation", "opening"] {
        let f = Case::new().await;
        if stage == "creation" {
            let mut creating = Box::pin(f.services.playback.begin_lease(&f.actor, f.request()));
            assert!(poll_once(creating.as_mut()).await.is_pending());
            let entry = f
                .services
                .playback
                .leases
                .state
                .lock()
                .unwrap()
                .entries
                .values()
                .next()
                .unwrap()
                .clone();
            entry.publications.created.notified().await;
            f.services.playback.close().await.unwrap();
            rejected.push((stage, creating.await.is_err()));
        } else {
            let lease = f
                .services
                .playback
                .begin_lease(&f.actor, f.request())
                .await
                .unwrap();
            let entry = f
                .services
                .playback
                .leases
                .entry(&f.actor, lease.id)
                .unwrap();
            let mut opening = Box::pin(f.services.playback.open_lease_audio(
                &f.actor,
                lease.id,
                AudioRequest::default(),
            ));
            hold_until_reply(opening.as_mut(), &entry.publications.opened).await;
            f.services.playback.close().await.unwrap();
            rejected.push((stage, opening.await.is_err()));
        }
        f.close().await;
    }
    assert!(
        rejected.iter().all(|(_, rejected)| *rejected),
        "reply handoffs must reject retired leases: {rejected:?}"
    );
}

async fn hold_until_reply<F: Future>(mut request: Pin<&mut F>, published: &Notify) {
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            if published.notified().now_or_never().is_some() {
                return;
            }
            assert!(
                poll_once(request.as_mut()).await.is_pending(),
                "the producer must reach the reply barrier before the consumer completes"
            );
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("owned reply must reach its publication barrier");
}
