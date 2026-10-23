use super::*;

#[tokio::test]
async fn unapproved_media_origins_fail_before_a_media_request() {
    for target in [
        "https://unapproved.example.invalid/owned-private-media-ticket",
        "http://media.hito-fixture.invalid/owned-private-media-ticket",
        "https://owned-private-user:owned-private-token@media.hito-fixture.invalid/owned-audio",
        "https://media.hito-fixture.invalid/owned-audio#owned-private-fragment",
    ] {
        let mut value = item();
        value["link"] = target.into();
        let journey = Journey::start(
            vec![json(&transfers()), json(&value)],
            vec![Response::wave(owned_wave())],
        )
        .await;
        let capture = Capture::default();
        rejected(
            journey
                .client
                .open_bound_audio(&binding(), &journey.allowlist)
                .with_subscriber(capture.dispatch())
                .await,
            Error::MediaDestination,
        );
        journey.close(2, 0).await;
        logs(&capture, &[Expected::Opening("media_destination")]);
    }
}

#[tokio::test]
async fn media_redirect_and_inconsistent_framing_never_become_audio() {
    let mut cases = Vec::new();
    let mut redirect = Response::wave(owned_wave());
    redirect.status = 302;
    redirect.headers.push((
        "Location".into(),
        "https://unapproved.example.invalid/owned-private-redirect".into(),
    ));
    cases.push(redirect);
    let mut length = Response::wave(owned_wave());
    length.headers[1].1 = (binding().file_bytes - 1).to_string();
    cases.push(length);
    let mut range = Response::wave(owned_wave());
    range.headers.push((
        "Content-Range".into(),
        format!(
            "bytes 0-{}/{}",
            binding().file_bytes - 1,
            binding().file_bytes
        ),
    ));
    cases.push(range);
    let mut encoding = Response::wave(owned_wave());
    encoding
        .headers
        .push(("Content-Encoding".into(), "gzip".into()));
    cases.push(encoding);
    for response in cases {
        let journey = Journey::start(vec![json(&transfers()), json(&item())], vec![response]).await;
        let capture = Capture::default();
        rejected(
            journey
                .client
                .open_bound_audio(&binding(), &journey.allowlist)
                .with_subscriber(capture.dispatch())
                .await,
            Error::MediaResponse,
        );
        journey.close(2, 1).await;
        logs(&capture, &[Expected::Opening("media_response")]);
    }
}

#[tokio::test]
async fn canonical_bytes_instead_of_mime_determine_supported_audio() {
    for content_type in ["audio/wav", "text/html"] {
        let body = b"<html>owned-private-non-audio-provider-body</html>".repeat(4);
        let mut configured = binding();
        configured.file_bytes = body.len() as u64;
        let mut value = item();
        value["size"] = configured.file_bytes.into();
        let response = super::super::fixture::Reply { content_type, body }.into();
        let journey = Journey::start(vec![json(&transfers()), json(&value)], vec![response]).await;
        let capture = Capture::default();
        rejected(
            journey
                .client
                .open_bound_audio(&configured, &journey.allowlist)
                .with_subscriber(capture.dispatch())
                .await,
            Error::UnsupportedAudio,
        );
        journey.close(2, 1).await;
        logs(&capture, &[Expected::Opening("unsupported_audio")]);
    }
    let mut response = Response::wave(owned_wave());
    response.headers[0].1 = "application/octet-stream".into();
    let journey = Journey::start(vec![json(&transfers()), json(&item())], vec![response]).await;
    let capture = Capture::default();
    async {
        let body = journey
            .client
            .open_bound_audio(&binding(), &journey.allowlist)
            .await
            .unwrap();
        collect(body, &binding()).await;
    }
    .with_subscriber(capture.dispatch())
    .await;
    journey.close(2, 1).await;
    logs(
        &capture,
        &[
            Expected::Opening("complete"),
            Expected::Body("complete", binding().file_bytes),
        ],
    );
}

#[tokio::test]
async fn truncated_valid_pcm_prefix_cannot_report_a_complete_body() {
    let mut response = Response::wave(owned_wave());
    response.body.truncate(44);
    let journey = Journey::start(vec![json(&transfers()), json(&item())], vec![response]).await;
    let capture = Capture::default();
    async {
        let mut body = journey
            .client
            .open_bound_audio(&binding(), &journey.allowlist)
            .await
            .unwrap();
        assert_eq!(body.next_chunk().await.unwrap().unwrap().len(), 44);
        assert!(matches!(
            body.next_chunk().await,
            Err(Error::MediaTransport)
        ));
        assert!(body.next_chunk().await.unwrap().is_none());
    }
    .with_subscriber(capture.dispatch())
    .await;
    journey.close(2, 1).await;
    logs(
        &capture,
        &[
            Expected::Opening("complete"),
            Expected::Body("media_transport", 44),
        ],
    );
}
