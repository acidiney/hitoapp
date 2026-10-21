#[path = "support/boundary.rs"]
#[allow(dead_code)]
mod boundary;
#[path = "support/mp3_boundaries.rs"]
mod mp3_boundaries;
#[path = "support/mp3_fixture.rs"]
mod mp3_fixture;
#[path = "support/mp3_profiles.rs"]
mod mp3_profiles;
#[allow(dead_code)]
mod support;

use futures_util::{FutureExt, future};
use hito_debrid_real_debrid::{MediaAllowlist, test_support::owned_protocol_client};
use hito_plugin_real_debrid::{Config, CuratedAudioRecording, router};
use hito_plugin_sdk::SourcePage;
use mp3_fixture::{decode, fixture};
use reqwest::{Method, StatusCode};
use serde_json::json;
use std::{panic::AssertUnwindSafe, time::Duration};
use support::{
    API_HOST, ARTIST, COOKIE, EXPECTED_FORM, HASH, HOSTER_LINK, MEDIA_HOST, MEDIA_TARGET,
    PROVIDER_TOKEN, RECORDING, SERVICE_TOKEN, SOURCE, TITLE, TORRENT, WRONG_TOKEN, http,
    logging::Capture,
    provider::{Counts, Origin, Provider},
};

const FILE: &str = "/private-owned-directory/private-owned-file-canary.mp3";

#[tokio::test]
async fn authenticated_mp3_audio_retains_original_bytes_and_decodes_through_eof() {
    let encoded = fixture().await;
    let expected_pcm = decode(&encoded);
    let provider = Provider::start_audio(encoded.clone(), "audio/mpeg", FILE, None).await;
    let capture = Capture::default();
    let mut plugin = None;
    let outcome = AssertUnwindSafe(tokio::time::timeout(Duration::from_secs(20), async {
        let mut binding = support::binding();
        binding.file_path = FILE.into();
        binding.file_bytes = encoded.len() as u64;
        let config = Config::new(
            SERVICE_TOKEN.into(),
            owned_protocol_client(
                PROVIDER_TOKEN.into(),
                provider.ca_der(),
                provider.api_address,
                provider.media_address,
            )
            .expect("owned provider transport"),
            MediaAllowlist::new(vec![format!("https://{MEDIA_HOST}")]).expect("owned media origin"),
            vec![CuratedAudioRecording {
                recording: support::recording(),
                source_id: SOURCE.into(),
                label: "Owned MP3".into(),
                binding,
                audio_kind: hito_debrid_real_debrid::AudioKind::Mp3,
            }],
        )
        .expect("the exact reviewed MP3 binding must be accepted");
        plugin = Some(
            http::Server::start(
                router(config).expect("owned plugin router"),
                capture.dispatch(),
            )
            .await,
        );
        let plugin = plugin.as_ref().expect("owned plugin exists");
        let client = reqwest::Client::builder()
            .no_proxy()
            .redirect(reqwest::redirect::Policy::none())
            .retry(reqwest::retry::never())
            .timeout(Duration::from_secs(4))
            .build()
            .expect("owned HTTP client");
        let audio_url = plugin
            .base
            .join(&format!("v1/audio?source_id={SOURCE}"))
            .expect("audio route");
        let rejected = client
            .get(audio_url.clone())
            .bearer_auth(WRONG_TOKEN)
            .header("x-hito-plugin-protocol", "1")
            .send()
            .await
            .expect("unauthorized request");
        assert_eq!(rejected.status(), StatusCode::UNAUTHORIZED);
        assert_eq!(
            provider.counts(),
            Counts {
                observations: 0,
                posts: 0,
                media: 0
            }
        );
        let (status, bytes) = http::json(
            &client,
            plugin,
            Method::POST,
            "v1/sources",
            Some(SERVICE_TOKEN),
            Some(json!({"recording_id": RECORDING})),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        let sources: SourcePage = serde_json::from_slice(&bytes).expect("typed source metadata");
        sources.validate().expect("source metadata must validate");
        assert_eq!(sources.items.len(), 1);
        assert!(sources.items[0].id == SOURCE && sources.items[0].ready);
        assert_eq!(
            provider.counts(),
            Counts {
                observations: 1,
                posts: 0,
                media: 0
            }
        );
        let head = client
            .head(audio_url.clone())
            .bearer_auth(SERVICE_TOKEN)
            .header("x-hito-plugin-protocol", "1")
            .send()
            .await
            .expect("metadata-only HEAD");
        assert_eq!(head.status(), StatusCode::OK);
        let head_headers = head.headers().clone();
        assert!(head.bytes().await.expect("HEAD body").is_empty());
        assert_eq!(
            provider.counts(),
            Counts {
                observations: 2,
                posts: 0,
                media: 0
            }
        );
        let mut response = client
            .get(audio_url)
            .bearer_auth(SERVICE_TOKEN)
            .header("x-hito-plugin-protocol", "1")
            .header("cookie", COOKIE)
            .send()
            .await
            .expect("explicit MP3 opening");
        let status = response.status();
        let headers = response.headers().clone();
        let mut bytes = Vec::new();
        while let Some(chunk) = response.chunk().await.expect("audio must finish") {
            assert!(
                chunk.len() <= encoded.len().saturating_sub(bytes.len()),
                "exact encoded length bound"
            );
            bytes.extend_from_slice(&chunk);
        }
        assert_eq!(
            provider.counts(),
            Counts {
                observations: 3,
                posts: 1,
                media: 1
            }
        );
        (status, head_headers, headers, bytes)
    }))
    .catch_unwind()
    .await;
    let close_plugin = async {
        match plugin {
            Some(plugin) => plugin.close().await,
            None => Ok(()),
        }
    };
    let (plugin_stopped, provider_stopped) = future::join(close_plugin, provider.close()).await;
    plugin_stopped.expect("plugin shutdown must join");
    let exchanges = provider_stopped.expect("owned TLS shutdown must join");
    let (status, head, headers, bytes) = match outcome {
        Ok(result) => result.expect("MP3 HTTP tracer must finish"),
        Err(panic) => std::panic::resume_unwind(panic),
    };
    assert_eq!(
        status,
        StatusCode::OK,
        "a natively decoded owned MP3 must relay over HTTP"
    );
    for headers in [&head, &headers] {
        assert_eq!(headers["content-type"], "audio/mpeg");
        assert_eq!(
            headers["content-length"]
                .to_str()
                .expect("audio length")
                .parse::<usize>()
                .expect("decimal length"),
            encoded.len()
        );
        assert_eq!(headers["cache-control"], "private, no-store");
        for name in [
            "location",
            "set-cookie",
            "content-disposition",
            "etag",
            "content-range",
            "accept-ranges",
        ] {
            assert!(
                headers.get(name).is_none(),
                "private upstream headers must remain private"
            );
        }
    }
    assert!(
        bytes == encoded,
        "the relay must preserve every original MP3 byte"
    );
    assert!(
        decode(&bytes) == expected_pcm,
        "native decoding through EOF must preserve exact PCM"
    );
    assert_eq!(exchanges.len(), 5);
    for exchange in &exchanges[..3] {
        assert!(exchange.origin == Origin::Api && exchange.method == "GET");
        assert!(exchange.target == format!("/rest/1.0/torrents/info/{TORRENT}"));
    }
    assert!(exchanges[3].origin == Origin::Api && exchanges[3].method == "POST");
    assert!(exchanges[3].target == "/rest/1.0/unrestrict/link");
    assert!(exchanges[3].body.as_slice() == EXPECTED_FORM.as_bytes());
    for exchange in &exchanges[..4] {
        assert!(exchange.server_name == API_HOST && exchange.headers["host"] == API_HOST);
        assert!(exchange.headers["authorization"] == format!("Bearer {PROVIDER_TOKEN}"));
        assert!(exchange.headers.get("cookie").is_none());
    }
    let media = &exchanges[4];
    assert!(media.origin == Origin::Media && media.method == "GET" && media.target == MEDIA_TARGET);
    assert_eq!(media.headers["accept"], "audio/mpeg");
    assert!(media.server_name == MEDIA_HOST && media.headers["host"] == MEDIA_HOST);
    assert!(media.body.is_empty());
    for name in [
        "authorization",
        "cookie",
        "proxy-authorization",
        "x-hito-plugin-protocol",
        "range",
    ] {
        assert!(
            media.headers.get(name).is_none(),
            "private caller headers must not enter media transport"
        );
    }
    capture.assert_private_completion(
        &[
            PROVIDER_TOKEN,
            SERVICE_TOKEN,
            WRONG_TOKEN,
            COOKIE,
            TITLE,
            ARTIST,
            TORRENT,
            HASH,
            FILE,
            RECORDING,
            SOURCE,
            "Owned MP3",
            HOSTER_LINK,
            MEDIA_TARGET,
            EXPECTED_FORM,
            "http://",
            "https://",
        ],
        encoded.len() as u64,
    );
}
