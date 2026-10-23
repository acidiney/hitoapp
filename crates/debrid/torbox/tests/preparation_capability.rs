#![cfg(feature = "test-support")]

pub use hito_debrid_torbox::test_support::fixtures;
pub use hito_debrid_torbox::test_support::logs;
pub use hito_debrid_torbox::test_support::owned_tls;
pub use hito_debrid_torbox::test_support::provider as protocol;
#[path = "support/capability_provider.rs"]
mod provider;

use fixtures::{TOKEN, binding};
use hito_debrid_torbox::{Error, TorBoxClient};
use logs::Capture;
use provider::CapabilityProvider;
use std::time::Duration;
use tracing::instrument::WithSubscriber;

#[tokio::test]
async fn verified_preparation_waits_for_explicit_mint_without_rechecking_or_reminting() {
    tokio::time::timeout(Duration::from_secs(5), async {
        let provider = CapabilityProvider::start(Duration::ZERO).await;
        let client =
            TorBoxClient::with_test_tls(TOKEN.into(), provider.ca_der(), provider.address())
                .unwrap();
        let configured = binding();
        let original = serde_json::to_vec(&configured).unwrap();
        let capture = Capture::default();

        let verified = client
            .verify_bound_file(&configured)
            .with_subscriber(capture.dispatch())
            .await
            .expect("verification must complete only the owned observation exchange");
        let before_journal = provider.counts();

        let candidate = verified
            .mint()
            .with_subscriber(capture.dispatch())
            .await
            .expect("an explicitly consumed verification must mint one candidate");

        assert!(candidate.binding() == &configured);
        assert!(serde_json::to_vec(&configured).unwrap() == original);
        drop(candidate);
        drop(client);
        let final_counts = provider.close().await;
        assert_eq!(final_counts, (2, 1));
        capture.assert_operations(
            &[
                ("torrent_observation", "provider_completed"),
                ("bound_file_preparation", "candidate"),
            ],
            &[TOKEN, &configured.info_hash, &configured.file_name],
        );
        println!(
            "Owned verified preparation before journal: provider_requests={} mint_requests={}",
            before_journal.0, before_journal.1
        );
        assert_eq!(
            before_journal,
            (1, 0),
            "verification must return before the caller authorizes any link mint"
        );
    })
    .await
    .expect("the owned TorBox verified preparation exceeded its deadline");
}

#[tokio::test]
async fn unpolled_mint_after_the_original_deadline_sends_no_mutation() {
    tokio::time::timeout(Duration::from_secs(15), async {
        let provider = CapabilityProvider::start(Duration::from_secs(1)).await;
        let client =
            TorBoxClient::with_test_tls(TOKEN.into(), provider.ca_der(), provider.address())
                .unwrap();
        let capture = Capture::default();
        let verified = client
            .verify_bound_file(&binding())
            .with_subscriber(capture.dispatch())
            .await
            .unwrap();
        assert_eq!(provider.counts(), (1, 0));
        let pending = verified.mint();
        tokio::time::sleep(Duration::from_millis(11_100)).await;

        assert!(matches!(pending.await, Err(Error::Deadline)));
        drop(client);
        assert_eq!(provider.close().await, (1, 0));
        capture.assert_operations(
            &[
                ("torrent_observation", "provider_completed"),
                ("bound_file_preparation", "deadline"),
            ],
            &[TOKEN],
        );
    })
    .await
    .expect("the owned TorBox expired-capability workflow exceeded its deadline");
}

#[tokio::test]
async fn dropping_verification_or_an_unpolled_mint_sends_no_mutation() {
    tokio::time::timeout(Duration::from_secs(5), async {
        let provider = CapabilityProvider::start(Duration::ZERO).await;
        let client =
            TorBoxClient::with_test_tls(TOKEN.into(), provider.ca_der(), provider.address())
                .unwrap();
        let capture = Capture::default();
        for unpolled_mint in [false, true] {
            let verified = client
                .verify_bound_file(&binding())
                .with_subscriber(capture.dispatch())
                .await
                .unwrap();
            if unpolled_mint {
                drop(verified.mint());
            } else {
                drop(verified);
            }
        }

        drop(client);
        assert_eq!(provider.close().await, (2, 0));
        capture.assert_operations(
            &[
                ("torrent_observation", "provider_completed"),
                ("bound_file_preparation", "cancelled"),
                ("torrent_observation", "provider_completed"),
                ("bound_file_preparation", "cancelled"),
            ],
            &[TOKEN],
        );
    })
    .await
    .expect("the owned TorBox dropped-capability workflow exceeded its deadline");
}

#[tokio::test]
async fn retained_verifications_share_real_control_admission_and_drop_releases_it() {
    tokio::time::timeout(Duration::from_secs(5), async {
        let provider = CapabilityProvider::start(Duration::ZERO).await;
        let client =
            TorBoxClient::with_test_tls(TOKEN.into(), provider.ca_der(), provider.address())
                .unwrap();
        let sibling = client.clone();
        let first_log = Capture::default();
        let second_log = Capture::default();
        let busy_log = Capture::default();
        let recovery_log = Capture::default();
        let first = client
            .verify_bound_file(&binding())
            .with_subscriber(first_log.dispatch())
            .await
            .unwrap();
        let second = sibling
            .verify_bound_file(&binding())
            .with_subscriber(second_log.dispatch())
            .await
            .unwrap();
        assert_eq!(provider.counts(), (2, 0));
        assert!(matches!(
            client
                .verify_bound_file(&binding())
                .with_subscriber(busy_log.dispatch())
                .await,
            Err(Error::Busy)
        ));
        assert_eq!(provider.counts(), (2, 0));

        drop(first);
        let replacement = client
            .verify_bound_file(&binding())
            .with_subscriber(recovery_log.dispatch())
            .await
            .unwrap();
        assert_eq!(provider.counts(), (3, 0));
        let second = second.mint().await.unwrap();
        let replacement = replacement.mint().await.unwrap();
        assert!(second.binding() == &binding() && replacement.binding() == &binding());
        drop(client);
        drop(sibling);
        assert_eq!(provider.close().await, (5, 2));
        first_log.assert_operations(
            &[
                ("torrent_observation", "provider_completed"),
                ("bound_file_preparation", "cancelled"),
            ],
            &[TOKEN],
        );
        busy_log.assert_operations(
            &[
                ("torrent_observation", "busy"),
                ("bound_file_preparation", "busy"),
            ],
            &[TOKEN],
        );
        for capture in [second_log, recovery_log] {
            capture.assert_operations(
                &[
                    ("torrent_observation", "provider_completed"),
                    ("bound_file_preparation", "candidate"),
                ],
                &[TOKEN],
            );
        }
    })
    .await
    .expect("the owned TorBox control-admission workflow exceeded its deadline");
}

#[tokio::test]
async fn cancelled_dispatched_capability_mint_is_unknown_and_closes_its_peer() {
    tokio::time::timeout(Duration::from_secs(7), async {
        let (mut held, received, _release) =
            protocol::reply::Reply::paused(fixtures::acknowledgment());
        let closed = held.observe_peer_close();
        let provider = protocol::Fixture::responses(vec![
            protocol::reply::Reply::json(fixtures::observed(true, true, "album/owned.wav")),
            held,
            protocol::reply::Reply::json(fixtures::observed(true, true, "album/owned.wav")),
        ])
        .await;
        let client =
            TorBoxClient::with_test_tls(TOKEN.into(), provider.ca_der(), provider.address())
                .unwrap();
        let capture = Capture::default();
        let verified = client
            .verify_bound_file(&binding())
            .with_subscriber(capture.dispatch())
            .await
            .unwrap();
        let mut pending = Box::pin(verified.mint().with_subscriber(capture.dispatch()));
        tokio::select! {
            _ = &mut pending => panic!("the owned mint must remain held before acknowledgement"),
            signal = tokio::time::timeout(Duration::from_secs(2), received) => {
                signal.unwrap().unwrap();
            }
        }
        drop(pending);
        tokio::time::timeout(Duration::from_secs(2), closed)
            .await
            .unwrap()
            .unwrap();
        client
            .observe(&binding())
            .with_subscriber(capture.dispatch())
            .await
            .unwrap();
        drop(client);
        let requests = provider.close().await;
        assert_eq!(requests.len(), 3);
        assert_eq!(
            requests
                .iter()
                .filter(|request| request.target.starts_with("/v1/api/torrents/requestdl?"))
                .count(),
            1
        );
        capture.assert_operations(
            &[
                ("torrent_observation", "provider_completed"),
                ("bound_file_preparation", "link_unknown"),
                ("torrent_observation", "provider_completed"),
            ],
            &[TOKEN],
        );
    })
    .await
    .expect("the owned TorBox uncertain-mint workflow exceeded its deadline");
}

#[tokio::test]
async fn caller_pause_and_dispatched_mint_share_the_original_deadline() {
    tokio::time::timeout(Duration::from_secs(15), async {
        let (mut held, received, _release) =
            protocol::reply::Reply::paused(fixtures::acknowledgment());
        let closed = held.observe_peer_close();
        let provider = protocol::Fixture::responses(vec![
            protocol::reply::Reply::json(fixtures::observed(true, true, "album/owned.wav")),
            held,
        ])
        .await;
        let client =
            TorBoxClient::with_test_tls(TOKEN.into(), provider.ca_der(), provider.address())
                .unwrap();
        let capture = Capture::default();
        let started = tokio::time::Instant::now();
        let verified = client
            .verify_bound_file(&binding())
            .with_subscriber(capture.dispatch())
            .await
            .unwrap();
        tokio::time::sleep(Duration::from_secs(10)).await;
        let mut pending = Box::pin(verified.mint().with_subscriber(capture.dispatch()));
        tokio::select! {
            _ = &mut pending => panic!("the owned mint must remain held before acknowledgement"),
            signal = tokio::time::timeout(Duration::from_secs(2), received) => {
                signal.unwrap().unwrap();
            }
        }
        let result = tokio::time::timeout(Duration::from_secs(3), pending)
            .await
            .expect("mint must retain the original remaining preparation budget");
        assert!(matches!(result, Err(Error::LinkUnknown)));
        assert!(started.elapsed() < Duration::from_secs(13));
        tokio::time::timeout(Duration::from_secs(2), closed)
            .await
            .unwrap()
            .unwrap();
        drop(client);
        assert_eq!(provider.close().await.len(), 2);
        capture.assert_operations(
            &[
                ("torrent_observation", "provider_completed"),
                ("bound_file_preparation", "link_unknown"),
            ],
            &[TOKEN],
        );
    })
    .await
    .expect("the owned TorBox aggregate-deadline workflow exceeded its deadline");
}
