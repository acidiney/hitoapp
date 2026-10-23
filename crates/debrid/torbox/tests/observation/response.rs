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
    let mut wrong_type = Reply::json(observation(true, true, "album/owned.wav"));
    wrong_type.headers[0].1 = "text/html".into();
    replies.push(wrong_type);
    let mut encoded = Reply::json(observation(true, true, "album/owned.wav"));
    encoded
        .headers
        .push(("Content-Encoding".into(), "gzip".into()));
    replies.push(encoded);
    let mut duplicate_type = Reply::json(observation(true, true, "album/owned.wav"));
    duplicate_type
        .headers
        .push(("Content-Type".into(), "application/json".into()));
    replies.push(duplicate_type);
    let mut oversized_head = Reply::json(observation(true, true, "album/owned.wav"));
    oversized_head
        .headers
        .push(("x-owned-private-header".into(), "private".repeat(1500)));
    replies.push(oversized_head);
    let fixture = Fixture::responses(replies).await;
    let client =
        TorBoxClient::with_test_tls(TOKEN.into(), fixture.ca_der(), fixture.address()).unwrap();
    let capture = Capture::default();
    for status in [302, 401, 429] {
        assert!(
            matches!(client.observe(&configured_binding()).with_subscriber(capture.dispatch()).await,
            Err(Error::Status(actual)) if actual == status)
        );
    }
    for _ in 0..4 {
        assert!(matches!(
            client
                .observe(&configured_binding())
                .with_subscriber(capture.dispatch())
                .await,
            Err(Error::InvalidResponse)
        ));
    }
    assert_eq!(fixture.close().await.len(), 7);
    capture.assert_observations(
        &[
            ("http_rejected", 0),
            ("http_rejected", 0),
            ("http_rejected", 0),
            ("invalid_response", 0),
            ("invalid_response", 0),
            ("invalid_response", 0),
            ("invalid_response", 0),
        ],
        &[TOKEN, "private", "album/owned.wav"],
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
    let mut exact = observation(true, true, "album/owned.wav");
    exact.resize(65_536, b' ');
    let mut exact = Reply::json(exact);
    exact.headers[0].1 = "Application/JSON; charset=utf-8".into();
    let fixture = Fixture::responses(vec![declared, streamed, exact]).await;
    let client =
        TorBoxClient::with_test_tls(TOKEN.into(), fixture.ca_der(), fixture.address()).unwrap();
    let capture = Capture::default();
    for _ in 0..2 {
        assert!(matches!(
            client
                .observe(&configured_binding())
                .with_subscriber(capture.dispatch())
                .await,
            Err(Error::ResponseTooLarge)
        ));
    }
    let result = client
        .observe(&configured_binding())
        .with_subscriber(capture.dispatch())
        .await
        .unwrap();
    assert_eq!(result.phase, TorrentPhase::ProviderCompleted);
    assert_eq!(fixture.close().await.len(), 3);
    capture.assert_observations(
        &[
            ("response_too_large", 0),
            ("response_too_large", 0),
            ("provider_completed", 1),
        ],
        &[TOKEN, "album/owned.wav"],
    );
}

#[tokio::test]
async fn malformed_missing_and_mistyped_provider_fields_cannot_qualify_completion() {
    let mut replies = vec![
        b"not owned JSON".to_vec(),
        br#"{"success":false,"error":"private-provider-error","data":null}"#.to_vec(),
    ];
    for mutation in 0..6 {
        let mut response: serde_json::Value =
            serde_json::from_slice(&observation(true, true, "album/owned.wav")).unwrap();
        match mutation {
            0 => response["data"] = serde_json::Value::Null,
            1 => response["data"] = json!([]),
            2 => response["data"]["download_finished"] = json!(1),
            3 => response["data"]
                .as_object_mut()
                .unwrap()
                .remove("download_present")
                .map(|_| ())
                .unwrap(),
            4 => response["data"]["files"] = json!("private wrong file shape"),
            5 => response["data"]["size"] = json!(-1200),
            _ => unreachable!(),
        }
        replies.push(serde_json::to_vec(&response).unwrap());
    }
    let fixture = Fixture::start(replies).await;
    let client =
        TorBoxClient::with_test_tls(TOKEN.into(), fixture.ca_der(), fixture.address()).unwrap();
    let capture = Capture::default();
    let mut expected = Vec::new();
    for index in 0..8 {
        let result = client
            .observe(&configured_binding())
            .with_subscriber(capture.dispatch())
            .await;
        if index == 1 {
            assert!(matches!(result, Err(Error::ProviderRejected)));
            expected.push(("provider_rejected", 0));
        } else {
            assert!(matches!(result, Err(Error::InvalidResponse)));
            expected.push(("invalid_response", 0));
        }
    }
    assert_eq!(fixture.close().await.len(), 8);
    capture.assert_observations(
        &expected,
        &[
            TOKEN,
            "private-provider-error",
            "private wrong file shape",
            "album/owned.wav",
        ],
    );
}
