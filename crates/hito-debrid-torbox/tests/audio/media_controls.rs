use super::*;

#[tokio::test]
async fn fresh_open_rejects_incomplete_and_changed_files_before_any_media_request() {
    for (finished, name, expected) in [
        (false, "album/owned.wav", Error::SourceIncomplete),
        (true, "album/replaced.wav", Error::SourceChanged),
    ] {
        let media = MediaFixture::start(wave()).await;
        let fixture = Fixture::start(vec![
            observed(true, true, "album/owned.wav"),
            acknowledgment(),
            observed(finished, true, name),
        ])
        .await;
        let client = TorBoxClient::with_test_tls_media(
            TOKEN.into(),
            fixture.ca_der(),
            fixture.address(),
            media.ca_der(),
            media.address(),
        )
        .unwrap();
        let candidate = client.prepare_bound_file(&binding()).await.unwrap();
        let capture = Capture::default();
        let result = client
            .open_bound_audio(
                candidate,
                &MediaAllowlist::new(vec![format!("https://{MEDIA_HOST}")]).unwrap(),
            )
            .with_subscriber(capture.dispatch())
            .await;
        assert!(matches!(result, Err(error) if error == expected));
        assert_eq!(fixture.close().await.len(), 3);
        assert!(media.close().await.is_empty());
        let outcome = if finished {
            "source_changed"
        } else {
            "incomplete"
        };
        capture.assert_operations(
            &[
                ("torrent_observation", outcome),
                ("bound_audio_opening", outcome),
            ],
            &[TOKEN, name],
        );
    }
}

#[tokio::test]
async fn media_origin_redirect_framing_and_wav_failures_use_shared_opener_policy() {
    for scenario in 0..7 {
        let mut reply = scripted_media::Reply::full(wave());
        match scenario {
            0 => reply.status = 302,
            1 => reply.content_type = "text/html",
            2 => reply.declared += 1,
            3 => reply.body[0] = b'X',
            4 => reply.body.truncate(44),
            5 => {}
            6 => {
                reply.content_type = "text/html";
                reply.body.fill(b' ');
                let html = b"<!DOCTYPE html><html>owned-private-non-audio-response";
                reply.body[..html.len()].copy_from_slice(html);
            }
            _ => unreachable!(),
        }
        let media = scripted_media::ScriptedMedia::start(vec![reply]).await;
        let fixture = Fixture::start(vec![
            observed(true, true, "album/owned.wav"),
            acknowledgment(),
            observed(true, true, "album/owned.wav"),
        ])
        .await;
        let client = TorBoxClient::with_test_tls_media(
            TOKEN.into(),
            fixture.ca_der(),
            fixture.address(),
            media.ca_der(),
            media.address(),
        )
        .unwrap();
        let candidate = client.prepare_bound_file(&binding()).await.unwrap();
        let capture = Capture::default();
        let origin = if scenario == 5 {
            "https://unapproved.invalid".into()
        } else {
            format!("https://{MEDIA_HOST}")
        };
        let result = client
            .open_bound_audio(candidate, &MediaAllowlist::new(vec![origin]).unwrap())
            .with_subscriber(capture.dispatch())
            .await;
        if scenario == 1 {
            let mut opened =
                result.expect("canonical WAV bytes qualify independently of misleading MIME");
            let mut bytes = Vec::new();
            while let Some(chunk) = opened.next_chunk().await.unwrap() {
                bytes.extend_from_slice(&chunk);
            }
            assert!(bytes == wave());
            assert_pcm(bytes);
            capture.assert_operations(
                &[
                    ("torrent_observation", "provider_completed"),
                    ("bound_audio_opening", "complete"),
                    ("bound_audio_opening", "complete"),
                ],
                &[TOKEN],
            );
        } else if scenario == 4 {
            let mut opened =
                result.expect("a complete canonical prefix may qualify before body failure");
            let mut delivered = 0;
            loop {
                match opened.next_chunk().await {
                    Ok(Some(chunk)) => delivered += chunk.len(),
                    Err(Error::MediaTransport) => break,
                    _ => panic!("truncated framed media must fail"),
                }
            }
            assert_eq!(delivered, 44);
            capture.assert_operations(
                &[
                    ("torrent_observation", "provider_completed"),
                    ("bound_audio_opening", "complete"),
                    ("bound_audio_opening", "media_transport"),
                ],
                &[TOKEN],
            );
        } else {
            let expected = match scenario {
                3 | 6 => Error::UnsupportedAudio,
                5 => Error::MediaDestination,
                _ => Error::MediaResponse,
            };
            assert_eq!(
                result.as_ref().err().copied(),
                Some(expected),
                "media control {scenario}"
            );
            let outcome = match scenario {
                3 | 6 => "unsupported_audio",
                5 => "media_destination",
                _ => "media_response",
            };
            capture.assert_operations(
                &[
                    ("torrent_observation", "provider_completed"),
                    ("bound_audio_opening", outcome),
                ],
                &[TOKEN],
            );
        }
        assert_eq!(fixture.close().await.len(), 3);
        assert_eq!(media.close().await, usize::from(scenario != 5));
    }
}
