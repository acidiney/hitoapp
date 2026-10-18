mod support;

use futures_util::{FutureExt, future};
use hito_debrid_real_debrid::{MediaAllowlist, test_support::owned_protocol_client};
use hito_plugin_real_debrid::{Config, CuratedWavRecording, router};
use hito_plugin_sdk::{
    Capability, Manifest, ProviderRecording, SearchPage, SourcePage, SourcePersistence,
};
use reqwest::{Method, StatusCode};
use serde_json::json;
use std::{panic::AssertUnwindSafe, time::Duration};
use support::{
    API_HOST, ARTIST, COOKIE, EXPECTED_FORM, FILE, HASH, HOSTER_LINK, LABEL, MEDIA_HOST,
    MEDIA_TARGET, PROVIDER_TOKEN, QUERY, RECORDING, SERVICE_TOKEN, SOURCE, TITLE, TORRENT,
    WRONG_TOKEN, audio, http,
    logging::Capture,
    provider::{Counts, Exchange, Origin, Provider},
};

#[tokio::test]
async fn authenticated_catalog_to_explicit_audio_decodes_the_exact_owned_wav() {
    let provider = Provider::start(audio::wav()).await;
    let capture = Capture::default();
    let mut plugin = None;
    let outcome = AssertUnwindSafe(tokio::time::timeout(Duration::from_secs(20), async {
        let provider_client = owned_protocol_client(
            PROVIDER_TOKEN.into(),
            provider.ca_der(),
            provider.api_address,
            provider.media_address,
        )
        .expect("the narrow fixed-host owned protocol client must initialize");
        let config = Config::new(
            SERVICE_TOKEN.into(),
            provider_client,
            MediaAllowlist::new(vec![format!("https://{MEDIA_HOST}")]).expect("owned media origin"),
            vec![CuratedWavRecording {
                recording: support::recording(),
                source_id: SOURCE.into(),
                label: LABEL.into(),
                binding: support::binding(),
            }],
        )
        .expect("the one-account curated canonical-WAV configuration must validate");
        let app = router(config).expect("the authenticated plugin router must initialize");
        plugin = Some(http::Server::start(app, capture.dispatch()).await);
        workflow(plugin.as_ref().expect("owned plugin must exist"), &provider).await
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
    plugin_stopped.expect("the actual plugin HTTP fixture must join shutdown");
    let exchanges = provider_stopped.expect("all owned TLS peers must join shutdown");
    let bytes = match outcome {
        Ok(result) => result.expect("the complete authenticated HTTP tracer must finish"),
        Err(panic) => std::panic::resume_unwind(panic),
    };
    assert!(
        bytes == audio::wav(),
        "HTTP must retain the complete exact owned WAV"
    );
    audio::assert_decoded(bytes);
    assert_protocol(&exchanges);
    capture.assert_private_completion(
        &[
            PROVIDER_TOKEN,
            SERVICE_TOKEN,
            WRONG_TOKEN,
            COOKIE,
            QUERY,
            TITLE,
            ARTIST,
            TORRENT,
            HASH,
            FILE,
            HOSTER_LINK,
            MEDIA_TARGET,
            EXPECTED_FORM,
            "http://",
            "https://",
        ],
        audio::WAV_BYTES as u64,
    );
}

async fn workflow(plugin: &http::Server, provider: &Provider) -> Vec<u8> {
    let client = reqwest::Client::builder()
        .no_proxy()
        .redirect(reqwest::redirect::Policy::none())
        .retry(reqwest::retry::never())
        .timeout(Duration::from_secs(4))
        .build()
        .expect("owned HTTP client");
    let (status, bytes) = http::json(&client, plugin, Method::GET, "v1/manifest", None, None).await;
    assert_eq!(status, StatusCode::OK);
    assert_private_metadata(&bytes);
    let manifest: Manifest =
        serde_json::from_slice(&bytes).expect("manifest must use SDK wire data");
    manifest.validate().expect("manifest must validate");
    assert_eq!(manifest.id, "org.hito.real-debrid");
    assert!(manifest.capabilities == [Capability::Search, Capability::Sources].into());
    assert_eq!(
        manifest
            .content_policy
            .expect("unqualified downloads must remain disabled")
            .source_persistence,
        SourcePersistence::PlaybackOnly
    );
    assert_eq!(
        provider.counts(),
        Counts {
            observations: 0,
            posts: 0,
            media: 0
        }
    );

    for token in [None, Some(WRONG_TOKEN)] {
        for (method, path, body) in [
            (
                Method::POST,
                "v1/search",
                Some(json!({"query": QUERY, "cursor": null, "limit": 10})),
            ),
            (
                Method::POST,
                "v1/recordings/detail",
                Some(json!({"recording_id": RECORDING})),
            ),
            (
                Method::POST,
                "v1/sources",
                Some(json!({"recording_id": RECORDING})),
            ),
            (Method::GET, "v1/audio?source_id=curated-wav-source-1", None),
            (
                Method::HEAD,
                "v1/audio?source_id=curated-wav-source-1",
                None,
            ),
        ] {
            let (status, bytes) = http::json(&client, plugin, method, path, token, body).await;
            assert_eq!(status, StatusCode::UNAUTHORIZED);
            assert_private_metadata(&bytes);
        }
    }
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
        "v1/search",
        Some(SERVICE_TOKEN),
        Some(json!({"query": QUERY, "cursor": null, "limit": 10})),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_private_metadata(&bytes);
    let page: SearchPage = serde_json::from_slice(&bytes).expect("search must use SDK wire data");
    page.validate(10).expect("bounded search must validate");
    assert!(page.items == vec![support::recording()]);
    assert!(page.next_cursor.is_none());

    let selection = json!({"recording_id": RECORDING});
    let (status, bytes) = http::json(
        &client,
        plugin,
        Method::POST,
        "v1/recordings/detail",
        Some(SERVICE_TOKEN),
        Some(selection.clone()),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_private_metadata(&bytes);
    let detail: ProviderRecording =
        serde_json::from_slice(&bytes).expect("detail must use SDK wire data");
    detail.validate().expect("curated detail must validate");
    assert!(detail == support::recording());
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
        Some(selection),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_private_metadata(&bytes);
    let sources: SourcePage =
        serde_json::from_slice(&bytes).expect("sources must use SDK wire data");
    sources.validate().expect("bounded sources must validate");
    assert_eq!(sources.items.len(), 1);
    assert_eq!(sources.items[0].id, SOURCE);
    assert_eq!(sources.items[0].label, LABEL);
    assert!(
        sources.items[0].ready,
        "the exact completed curated WAV must be eligible for explicit opening"
    );
    assert_eq!(
        provider.counts(),
        Counts {
            observations: 1,
            posts: 0,
            media: 0
        }
    );

    let url = plugin
        .base
        .join("v1/audio?source_id=curated-wav-source-1")
        .expect("owned audio route");
    let response = client
        .head(url.clone())
        .bearer_auth(SERVICE_TOKEN)
        .header("x-hito-plugin-protocol", "1")
        .send()
        .await
        .expect("owned HEAD must finish");
    assert_eq!(response.status(), StatusCode::OK);
    assert_audio_headers(response.headers());
    assert!(response.bytes().await.expect("HEAD must finish").is_empty());
    let before_audio = provider.counts();
    assert!(before_audio.observations <= 2);
    assert_eq!(
        before_audio.posts, 0,
        "HEAD must not unrestrict a provider link"
    );
    assert_eq!(before_audio.media, 0, "HEAD must not open media");

    let mut response = client
        .get(url)
        .bearer_auth(SERVICE_TOKEN)
        .header("x-hito-plugin-protocol", "1")
        .header("cookie", COOKIE)
        .send()
        .await
        .expect("explicit audio GET must open");
    assert_eq!(response.status(), StatusCode::OK);
    assert_audio_headers(response.headers());
    let mut bytes = Vec::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .expect("owned HTTP audio must reach EOF")
    {
        assert!(
            chunk.len() <= audio::WAV_BYTES.saturating_sub(bytes.len()),
            "audio exceeded its exact bound"
        );
        bytes.extend_from_slice(&chunk);
    }
    assert_eq!(bytes.len(), audio::WAV_BYTES);
    assert_eq!(
        provider.counts(),
        Counts {
            observations: before_audio.observations + 1,
            posts: 1,
            media: 1
        }
    );
    bytes
}

fn assert_audio_headers(headers: &reqwest::header::HeaderMap) {
    assert_eq!(headers["content-type"], "audio/wav");
    assert_eq!(
        headers["content-length"]
            .to_str()
            .expect("audio length header")
            .parse::<u64>()
            .expect("decimal audio length"),
        audio::WAV_BYTES as u64
    );
    assert_eq!(headers["cache-control"], "private, no-store");
    for private in [
        "location",
        "set-cookie",
        "content-disposition",
        "etag",
        "content-range",
        "accept-ranges",
    ] {
        assert!(
            headers.get(private).is_none(),
            "unqualified upstream headers must not escape"
        );
    }
}

fn assert_private_metadata(bytes: &[u8]) {
    let text = std::str::from_utf8(bytes).expect("metadata must be UTF-8");
    for forbidden in [
        PROVIDER_TOKEN,
        SERVICE_TOKEN,
        WRONG_TOKEN,
        COOKIE,
        TORRENT,
        HASH,
        FILE,
        HOSTER_LINK,
        MEDIA_TARGET,
        "http://",
        "https://",
    ] {
        assert!(
            !text.contains(forbidden),
            "metadata exposed private provider transport data"
        );
    }
}

fn assert_protocol(exchanges: &[Exchange]) {
    assert!((4..=5).contains(&exchanges.len()));
    let post = exchanges
        .iter()
        .position(|exchange| exchange.method == "POST")
        .expect("one unrestriction POST must exist");
    assert_eq!(
        post + 2,
        exchanges.len(),
        "unrestriction must be followed only by media GET"
    );
    for exchange in &exchanges[..post] {
        assert!(exchange.origin == Origin::Api && exchange.method == "GET");
        assert!(exchange.target == format!("/rest/1.0/torrents/info/{TORRENT}"));
        assert!(exchange.body.is_empty());
    }
    assert!(
        exchanges[post].origin == Origin::Api
            && exchanges[post].target == "/rest/1.0/unrestrict/link"
    );
    assert!(exchanges[post].body.as_slice() == EXPECTED_FORM.as_bytes());
    assert_eq!(
        exchanges[post].headers["content-type"],
        "application/x-www-form-urlencoded"
    );
    for exchange in &exchanges[..=post] {
        assert!(exchange.server_name == API_HOST && exchange.headers["host"] == API_HOST);
        assert!(exchange.headers["authorization"] == format!("Bearer {PROVIDER_TOKEN}"));
        assert_eq!(exchange.headers["accept"], "application/json");
    }
    let media = exchanges.last().expect("owned media request must exist");
    assert!(media.origin == Origin::Media && media.method == "GET" && media.target == MEDIA_TARGET);
    assert!(media.server_name == MEDIA_HOST && media.headers["host"] == MEDIA_HOST);
    assert!(media.body.is_empty());
    assert!(media.headers.get("authorization").is_none());
    for exchange in exchanges {
        for header in [
            "cookie",
            "proxy-authorization",
            "x-hito-plugin-protocol",
            "range",
            "transfer-encoding",
        ] {
            assert!(
                exchange.headers.get(header).is_none(),
                "caller headers must not enter provider transport"
            );
        }
        assert_eq!(exchange.headers["accept-encoding"], "identity");
        for value in exchange.headers.values() {
            let value = value.to_str().expect("owned headers must be ASCII");
            assert!(
                !value.contains(SERVICE_TOKEN)
                    && !value.contains(WRONG_TOKEN)
                    && !value.contains(COOKIE)
            );
        }
    }
}
