use crate::{
    FILE,
    boundary::{self, Fixture},
    mp3_fixture::{decode_profile, encoded, id3_footer},
    support::{
        self, API_HOST, MEDIA_HOST, SERVICE_TOKEN, SOURCE,
        provider::{Counts, Origin, Provider},
    },
};
use futures_util::FutureExt;
use hito_debrid_real_debrid::AudioKind;
use hito_plugin_real_debrid::CuratedAudioRecording;
use reqwest::{Method, StatusCode, header::HeaderMap};
use std::{panic::AssertUnwindSafe, time::Duration};

const LABEL: &str = "private-owned-mp3-profile-label-canary";

#[tokio::test]
async fn valid_mp3_profiles_relay_original_bytes_and_native_pcm_through_eof() {
    let tagless = encoded(2, Some(0)).await;
    assert!(
        !tagless.starts_with(b"ID3"),
        "tagless fixture must omit ID3"
    );
    let version3 = encoded(2, Some(3)).await;
    assert!(
        version3.starts_with(b"ID3\x03\x00"),
        "owned encoder must produce genuine ID3v2.3"
    );
    let footer = id3_footer(&tagless);
    let mono = encoded(1, Some(4)).await;
    assert!(
        mono.starts_with(b"ID3\x04\x00"),
        "owned mono encoder must produce ID3v2.4"
    );

    let mut qualified = Vec::with_capacity(4);
    for (profile, bytes, channels) in [
        ("tagless stereo", tagless, 2),
        ("ID3v2.3 stereo", version3, 2),
        ("ID3v2.4 footer stereo", footer, 2),
        ("ID3v2.4 mono", mono, 1),
    ] {
        eprintln!("Owned MP3 fixture validation: {profile}");
        let pcm = decode_profile(&bytes, channels);
        qualified.push((profile, bytes, channels, pcm));
    }
    assert!(
        qualified[0].3 == qualified[2].3,
        "footer metadata must preserve native PCM"
    );

    for (profile, bytes, channels, pcm) in qualified {
        eprintln!("Owned MP3 HTTP qualification: {profile}");
        relay(profile, bytes, channels, pcm).await;
    }
    println!("Owned MP3 profiles: native_validated=4 http_complete=4");
}

async fn relay(profile: &str, encoded: Vec<u8>, channels: u16, expected_pcm: Vec<f32>) {
    let mut binding = support::binding();
    binding.file_path = FILE.into();
    binding.file_bytes = encoded.len() as u64;
    let provider = Provider::start_audio(encoded.clone(), "audio/mpeg", FILE, None).await;
    let fixture = Fixture::with_provider(
        vec![CuratedAudioRecording {
            recording: support::recording(),
            source_id: SOURCE.into(),
            label: LABEL.into(),
            binding,
            audio_kind: AudioKind::Mp3,
        }],
        provider,
    )
    .await;
    let outcome = AssertUnwindSafe(tokio::time::timeout(Duration::from_secs(15), async {
        let path = format!("v1/audio?source_id={SOURCE}");
        let head = fixture
            .request(Method::HEAD, &path)
            .header("x-hito-plugin-protocol", "1")
            .send()
            .await
            .expect("profile HEAD must finish");
        assert_eq!(head.status(), StatusCode::OK);
        audio_headers(head.headers(), encoded.len());
        assert!(head.bytes().await.expect("profile HEAD body").is_empty());
        assert_eq!(
            fixture.provider.counts(),
            Counts {
                observations: 1,
                posts: 0,
                media: 0
            }
        );

        let mut response = fixture
            .request(Method::GET, &path)
            .header("x-hito-plugin-protocol", "1")
            .send()
            .await
            .expect("profile HTTP opening must finish");
        assert_eq!(
            response.status(),
            StatusCode::OK,
            "valid {profile} must relay over HTTP"
        );
        audio_headers(response.headers(), encoded.len());
        let mut received = Vec::new();
        while let Some(part) = response
            .chunk()
            .await
            .expect("profile audio must reach HTTP EOF")
        {
            assert!(
                part.len() <= encoded.len().saturating_sub(received.len()),
                "profile byte bound"
            );
            received.extend_from_slice(&part);
        }
        assert_eq!(
            fixture.provider.counts(),
            Counts {
                observations: 2,
                posts: 1,
                media: 1
            }
        );
        received
    }))
    .catch_unwind()
    .await;
    let (capture, exchanges) = fixture.close().await;
    let received = match outcome {
        Ok(result) => result.expect("owned profile HTTP workflow must meet its deadline"),
        Err(panic) => std::panic::resume_unwind(panic),
    };
    assert!(
        received == encoded,
        "{profile} must preserve every original encoded byte"
    );
    assert!(
        decode_profile(&received, channels) == expected_pcm,
        "{profile} must preserve native PCM through EOF"
    );
    assert_eq!(exchanges.len(), 4);
    for exchange in &exchanges[..2] {
        assert!(exchange.origin == Origin::Api && exchange.method == "GET");
        assert!(exchange.target == format!("/rest/1.0/torrents/info/{}", support::TORRENT));
    }
    assert!(exchanges[2].origin == Origin::Api && exchanges[2].method == "POST");
    assert!(exchanges[2].target == "/rest/1.0/unrestrict/link");
    assert!(exchanges[2].body.as_slice() == support::EXPECTED_FORM.as_bytes());
    for exchange in &exchanges[..3] {
        assert!(exchange.server_name == API_HOST && exchange.headers["host"] == API_HOST);
        assert!(exchange.headers["authorization"] == format!("Bearer {}", support::PROVIDER_TOKEN));
        assert!(exchange.headers.get("cookie").is_none());
    }
    let media = &exchanges[3];
    assert!(
        media.origin == Origin::Media
            && media.method == "GET"
            && media.target == support::MEDIA_TARGET
    );
    assert!(media.server_name == MEDIA_HOST && media.headers["host"] == MEDIA_HOST);
    assert_eq!(media.headers["accept"], "audio/mpeg");
    assert!(media.body.is_empty());
    for header in [
        "authorization",
        "cookie",
        "proxy-authorization",
        "x-hito-plugin-protocol",
        "range",
    ] {
        assert!(
            media.headers.get(header).is_none(),
            "profile caller headers must remain private"
        );
    }
    let mut canaries = boundary::private_log_canaries().to_vec();
    canaries.extend([FILE, LABEL, SOURCE, support::RECORDING, SERVICE_TOKEN]);
    capture.assert_private_completion(&canaries, encoded.len() as u64);
}

fn audio_headers(headers: &HeaderMap, length: usize) {
    assert_eq!(headers["content-type"], "audio/mpeg");
    assert_eq!(
        headers["content-length"]
            .to_str()
            .expect("profile length")
            .parse::<usize>()
            .expect("decimal profile length"),
        length
    );
    assert_eq!(headers["cache-control"], "private, no-store");
    for header in [
        "location",
        "set-cookie",
        "content-disposition",
        "etag",
        "content-range",
        "accept-ranges",
    ] {
        assert!(
            headers.get(header).is_none(),
            "profile upstream headers must remain private"
        );
    }
}
