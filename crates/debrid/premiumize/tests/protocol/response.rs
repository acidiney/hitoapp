use super::*;
use support::reply::Reply;

#[tokio::test]
async fn invalid_http_responses_and_redirects_do_not_expose_or_retry_private_data() {
    let mut replies = Vec::new();
    for status in [302, 401, 429] {
        let mut reply = Reply::json(b"private provider error response".to_vec());
        reply.status = status;
        reply.headers.push((
            "Location".into(),
            "https://private.example.invalid/do-not-follow?credential=private".into(),
        ));
        replies.push(reply);
    }
    let mut wrong_type = Reply::json(serde_json::to_vec(&transfer("finished", 1.0)).unwrap());
    wrong_type.headers[0].1 = "text/html".into();
    replies.push(wrong_type);
    let mut encoded = Reply::json(serde_json::to_vec(&transfer("finished", 1.0)).unwrap());
    encoded
        .headers
        .push(("Content-Encoding".into(), "gzip".into()));
    replies.push(encoded);
    let mut duplicate_type = Reply::json(serde_json::to_vec(&transfer("finished", 1.0)).unwrap());
    duplicate_type
        .headers
        .push(("Content-Type".into(), "application/json".into()));
    replies.push(duplicate_type);
    let mut oversized_head = Reply::json(serde_json::to_vec(&transfer("finished", 1.0)).unwrap());
    oversized_head
        .headers
        .push(("x-owned-private-header".into(), "private".repeat(1500)));
    replies.push(oversized_head);
    let fixture = Fixture::responses(replies).await;
    let client = client(&fixture);
    let capture = Capture::default();
    for status in [302, 401, 429] {
        assert!(
            matches!(client.observe(&binding()).with_subscriber(capture.dispatch()).await,
            Err(Error::Status(actual)) if actual == status)
        );
    }
    for _ in 0..4 {
        assert!(matches!(
            client
                .observe(&binding())
                .with_subscriber(capture.dispatch())
                .await,
            Err(Error::InvalidResponse)
        ));
    }
    assert_eq!(fixture.close().await.len(), 7);
    capture.assert_workflows(
        &[
            ("http_rejected", 0),
            ("http_rejected", 0),
            ("http_rejected", 0),
            ("invalid_response", 0),
            ("invalid_response", 0),
            ("invalid_response", 0),
            ("invalid_response", 0),
        ],
        &[TOKEN, "private", TRANSFER_ID],
    );
}

#[tokio::test]
async fn declared_and_streamed_body_bounds_preserve_exact_boundary_json() {
    let mut declared = Reply::json(Vec::new());
    declared.headers[1].1 = "65537".into();
    let mut streamed =
        Reply::json(format!("10001\r\n{}\r\n0\r\n\r\n", " ".repeat(65_537)).into_bytes());
    streamed
        .headers
        .retain(|(name, _)| name != "Content-Length");
    streamed
        .headers
        .push(("Transfer-Encoding".into(), "chunked".into()));
    let mut exact = serde_json::to_vec(&transfer("finished", 1.0)).unwrap();
    exact.resize(65_536, b' ');
    let mut exact = Reply::json(exact);
    exact.headers[0].1 = "Application/JSON; charset=utf-8".into();
    let fixture = Fixture::responses(vec![declared, streamed, exact]).await;
    let client = client(&fixture);
    let capture = Capture::default();
    for _ in 0..2 {
        assert!(matches!(
            client
                .observe(&binding())
                .with_subscriber(capture.dispatch())
                .await,
            Err(Error::ResponseTooLarge)
        ));
    }
    let result = client
        .observe(&binding())
        .with_subscriber(capture.dispatch())
        .await
        .unwrap();
    assert_eq!(
        result.phase,
        hito_debrid_premiumize::TransferPhase::ProviderCompleted
    );
    assert_eq!(fixture.close().await.len(), 3);
    capture.assert_workflows(
        &[
            ("response_too_large", 0),
            ("response_too_large", 0),
            ("provider_completed", 1),
        ],
        &[TOKEN, TRANSFER_ID],
    );
}
