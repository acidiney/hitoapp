#[path = "audio/admission_controls.rs"]
mod admission_controls;
#[path = "audio/fixtures.rs"]
mod fixtures;
#[path = "audio/logs.rs"]
mod logs;
#[path = "audio/media_controls.rs"]
mod media_controls;
#[path = "audio/media_fixture.rs"]
mod media_fixture;
#[path = "audio/mint_controls.rs"]
mod mint_controls;
#[path = "audio/opening_controls.rs"]
mod opening_controls;
#[path = "audio/owned_tls.rs"]
mod owned_tls;
#[path = "audio/provider_fixture.rs"]
mod provider_fixture;
#[path = "audio/scripted_media.rs"]
mod scripted_media;
#[path = "support/mod.rs"]
pub mod support;

use fixtures::*;
use hito_audio::Decoder;
use hito_debrid_torbox::{Error, MediaAllowlist, TorBoxClient};
use logs::Capture;
use media_fixture::MediaFixture;
use serde_json::json;
use std::{io::Cursor, time::Duration};
use support::Fixture;
use tracing::instrument::WithSubscriber;

#[tokio::test]
async fn completed_file_is_revalidated_before_opaque_link_audio_reaches_exact_pcm_eof() {
    tokio::time::timeout(Duration::from_secs(20), async {
        let owned_wave = wave();
        let media = MediaFixture::start(owned_wave.clone()).await;
        let private_link = format!("https://{MEDIA_HOST}{MEDIA_TARGET}");
        let fixture = Fixture::start(vec![
            observed(true, true, "album/owned.wav"),
            serde_json::to_vec(&json!({ "success": true, "error": null,
                "detail": "owned-private-link-detail", "data": private_link })).unwrap(),
            observed(true, true, "album/owned.wav"),
            observed(false, true, "album/owned.wav"),
            observed(true, true, "album/replaced.wav"),
        ]).await;
        let client = TorBoxClient::with_test_tls_media(TOKEN.into(), fixture.ca_der(), fixture.address(),
            media.ca_der(), media.address()).expect("the owned API and media authorities must configure");
        let allowlist = MediaAllowlist::new(vec![format!("https://{MEDIA_HOST}")]).unwrap();
        let configured = binding();
        let original = serde_json::to_vec(&configured).unwrap();
        let capture = Capture::default();
        let candidate = client.prepare_bound_file(&configured).with_subscriber(capture.dispatch()).await
            .expect("an exact completed file must return one opaque acknowledged candidate");
        assert!(candidate.binding() == &configured);
        assert_eq!(media.requests(), 0, "preparation must not fetch media");
        let mut opened = client.open_bound_audio(candidate, &allowlist).with_subscriber(capture.dispatch()).await
            .expect("fresh exact binding must qualify the owned PCM16 WAV");
        assert!(opened.binding == configured);
        assert_eq!(opened.length, WAV_BYTES);
        assert_eq!((opened.format.sample_rate, opened.format.channels, opened.format.frames),
            (SAMPLE_RATE, CHANNELS, FRAMES));
        let mut bytes = Vec::new();
        while let Some(chunk) = opened.next_chunk().await.expect("the bound WAV must reach validated EOF") {
            assert!(!chunk.is_empty() && chunk.len() <= 65_536);
            assert!(chunk.len() <= (WAV_BYTES as usize).saturating_sub(bytes.len()));
            bytes.extend_from_slice(&chunk);
        }
        assert!(bytes == owned_wave);
        assert!(serde_json::to_vec(&configured).unwrap() == original);
        assert!(matches!(client.prepare_bound_file(&configured).with_subscriber(capture.dispatch()).await,
            Err(Error::SourceIncomplete)));
        assert!(matches!(client.prepare_bound_file(&configured).with_subscriber(capture.dispatch()).await,
            Err(Error::SourceChanged)));
        assert!(serde_json::to_vec(&configured).unwrap() == original);
        let requests = fixture.close().await;
        let media_requests = media.close().await;
        assert_eq!(requests.len(), 5);
        assert_eq!(media_requests.len(), 1);
        for (index, request) in requests.iter().enumerate() {
            assert!(request.method == "GET" && request.server_name == "api.torbox.app");
            assert!(request.header("host") == Some("api.torbox.app"));
            assert!(request.header("accept") == Some("application/json"));
            assert!(request.header("accept-encoding") == Some("identity"));
            assert!(request.body.is_empty());
            for name in ["cookie", "referer", "range", "proxy-authorization", "content-type"] {
                assert!(request.header(name).is_none());
            }
            if index == 1 {
                assert!(request.target == "/v1/api/torrents/requestdl?token=owned-torbox-key%2F%2B%3F%3D%26-kept-private&torrent_id=7123&file_id=7&zip_link=false&redirect=false&append_name=false");
                assert!(request.header("authorization").is_none());
            } else {
                assert!(request.target == "/v1/api/torrents/mylist?id=7123&bypass_cache=true");
                assert!(request.header("authorization") == Some(format!("Bearer {TOKEN}").as_str()));
            }
        }
        let media = &media_requests[0];
        assert!(media.method == "GET" && media.server_name == MEDIA_HOST && media.target == MEDIA_TARGET);
        assert!(media.header("host") == Some(MEDIA_HOST));
        assert!(media.header("accept") == Some("audio/wav"));
        assert!(media.header("accept-encoding") == Some("identity"));
        assert!(media.received_at >= requests[2].received_at);
        for name in ["authorization", "cookie", "referer", "range", "proxy-authorization", "x-hito-plugin-protocol"] {
            assert!(media.header(name).is_none());
        }
        let mut decoder = Decoder::open(Cursor::new(bytes)).expect("the exact owned WAV must decode");
        let mut count = 0;
        while let Some(block) = decoder.read().expect("the decoder must finish at exact PCM EOF") {
            for value in block.samples {
                assert!((*value - f32::from(sample(count)) / 32768.0).abs() < 0.000_001);
                count += 1;
            }
        }
        assert_eq!(count as u64, FRAMES * u64::from(CHANNELS));
        capture.assert_flows(&[TOKEN, "owned-torbox-key%2F%2B%3F%3D%26-kept-private", &private_link,
            &configured.info_hash, &configured.file_name, "album/replaced.wav",
            "owned-private-observation-detail", "owned-private-link-detail", "owned-private-torrent-title"], WAV_BYTES);
    }).await.expect("the bounded owned TorBox-to-WAV tracer exceeded its deadline");
}
