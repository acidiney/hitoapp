use super::*;
use support::reply::Reply;

#[tokio::test]
async fn malformed_oversized_and_rejected_mint_acknowledgments_are_not_retried() {
    for scenario in 0..7 {
        let mut reply = match scenario {
            0 => Reply::json(b"owned-private-malformed-ack".to_vec()),
            1 => {
                let mut r = Reply::json(Vec::new());
                r.headers[1].1 = "65537".into();
                r
            }
            2 => {
                let mut r = Reply::json(b"private denied".to_vec());
                r.status = 401;
                r
            }
            3 => {
                let mut r = Reply::json(b"private denied".to_vec());
                r.status = 403;
                r
            }
            4 => Reply::json(
                br#"{"success":false,"error":"private provider rejection","data":null}"#.to_vec(),
            ),
            5 => Reply::json(br#"{"success":true,"error":null,"data":{}}"#.to_vec()),
            6 => {
                let mut r = Reply::json(b"private redirect".to_vec());
                r.status = 302;
                r.headers.push((
                    "Location".into(),
                    "https://private.invalid/do-not-follow".into(),
                ));
                r
            }
            _ => unreachable!(),
        };
        reply.headers.push((
            "x-owned-private-header".into(),
            "owned-private-mint-header".into(),
        ));
        let fixture = Fixture::responses(vec![
            Reply::json(observed(true, true, "album/owned.wav")),
            reply,
        ])
        .await;
        let client =
            TorBoxClient::with_test_tls(TOKEN.into(), fixture.ca_der(), fixture.address()).unwrap();
        let capture = Capture::default();
        let result = client
            .prepare_bound_file(&binding())
            .with_subscriber(capture.dispatch())
            .await;
        let expected = match scenario {
            2 => Error::Status(401),
            3 => Error::Status(403),
            4 => Error::ProviderRejected,
            _ => Error::LinkUnknown,
        };
        assert!(matches!(result, Err(error) if error==expected));
        assert_eq!(fixture.close().await.len(), 2);
        let outcome = match scenario {
            2 | 3 => "http_rejected",
            4 => "provider_rejected",
            _ => "link_unknown",
        };
        capture.assert_operations(
            &[
                ("torrent_observation", "provider_completed"),
                ("bound_file_preparation", outcome),
            ],
            &[
                TOKEN,
                "owned-private-malformed-ack",
                "private denied",
                "private provider rejection",
                "owned-private-mint-header",
            ],
        );
    }
}

#[tokio::test]
async fn cancelled_dispatched_mint_is_uncertain_and_closes_its_peer_without_retry() {
    let (mut held, received, _release) = Reply::paused(acknowledgment());
    let closed = held.observe_peer_close();
    let fixture = Fixture::responses(vec![
        Reply::json(observed(true, true, "album/owned.wav")),
        held,
    ])
    .await;
    let client =
        TorBoxClient::with_test_tls(TOKEN.into(), fixture.ca_der(), fixture.address()).unwrap();
    let observing = client.clone();
    let capture = Capture::default();
    let dispatch = capture.dispatch();
    let task = tokio::spawn(async move {
        observing
            .prepare_bound_file(&binding())
            .with_subscriber(dispatch)
            .await
    });
    tokio::time::timeout(Duration::from_secs(2), received)
        .await
        .unwrap()
        .unwrap();
    task.abort();
    assert!(matches!(task.await,Err(error) if error.is_cancelled()));
    tokio::time::timeout(Duration::from_secs(2), closed)
        .await
        .unwrap()
        .unwrap();
    drop(client);
    assert_eq!(fixture.close().await.len(), 2);
    capture.assert_operations(
        &[
            ("torrent_observation", "provider_completed"),
            ("bound_file_preparation", "link_unknown"),
        ],
        &[TOKEN],
    );
}

#[tokio::test]
async fn stalled_mint_deadline_is_uncertain_and_closes_its_peer_without_retry() {
    let (mut held, received, _release) = Reply::paused(acknowledgment());
    let closed = held.observe_peer_close();
    let fixture = Fixture::responses(vec![
        Reply::json(observed(true, true, "album/owned.wav")),
        held,
    ])
    .await;
    let client =
        TorBoxClient::with_test_tls(TOKEN.into(), fixture.ca_der(), fixture.address()).unwrap();
    let observing = client.clone();
    let capture = Capture::default();
    let dispatch = capture.dispatch();
    let task = tokio::spawn(async move {
        observing
            .prepare_bound_file(&binding())
            .with_subscriber(dispatch)
            .await
    });
    tokio::time::timeout(Duration::from_secs(2), received)
        .await
        .unwrap()
        .unwrap();
    let result = tokio::time::timeout(Duration::from_secs(12), task)
        .await
        .unwrap()
        .unwrap();
    assert!(matches!(result, Err(Error::LinkUnknown)));
    tokio::time::timeout(Duration::from_secs(2), closed)
        .await
        .unwrap()
        .unwrap();
    drop(client);
    assert_eq!(fixture.close().await.len(), 2);
    capture.assert_operations(
        &[
            ("torrent_observation", "provider_completed"),
            ("bound_file_preparation", "link_unknown"),
        ],
        &[TOKEN],
    );
}

#[tokio::test]
async fn lost_mint_headers_remain_uncertain_without_an_automatic_retry() {
    let fixture = provider_fixture::ProviderFixture::start(vec![
        Some(observed(true, true, "album/owned.wav")),
        None,
    ])
    .await;
    let client =
        TorBoxClient::with_test_tls(TOKEN.into(), fixture.ca_der(), fixture.address()).unwrap();
    let capture = Capture::default();
    let result = client
        .prepare_bound_file(&binding())
        .with_subscriber(capture.dispatch())
        .await;
    assert!(matches!(result, Err(Error::LinkUnknown)));
    assert_eq!(fixture.close().await, 2);
    capture.assert_operations(
        &[
            ("torrent_observation", "provider_completed"),
            ("bound_file_preparation", "link_unknown"),
        ],
        &[TOKEN],
    );
}
