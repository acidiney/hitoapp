#[path = "support/fixture.rs"]
mod fixture;
#[path = "support/logging.rs"]
mod logging;

use crate::{Config, router};
use fixture::{CLIENT_ID, Provider, Server};
use hito_plugin_sdk::{Capability, Manifest, ProviderRecording, SearchPage, SourcePage};
use serde_json::json;
use std::time::Duration;

#[tokio::test]
async fn search_to_explicit_audio_uses_typed_metadata_without_prefetching_media() {
    let provider = Provider::start().await;
    let config = Config::fixture(CLIENT_ID.to_owned(), provider.base.clone());
    let plugin = Server::start(router(config).expect("Jamendo router setup failed")).await;
    let client = reqwest::Client::builder()
        .no_proxy()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(Duration::from_secs(3))
        .build()
        .expect("Jamendo fixture client setup failed");

    let response = client
        .get(plugin.base.join("v1/manifest").unwrap())
        .send()
        .await
        .expect("manifest request failed");
    assert_eq!(response.status(), reqwest::StatusCode::OK);
    let manifest: Manifest = response.json().await.expect("invalid manifest JSON");
    manifest.validate().expect("invalid manifest contract");
    assert_eq!(manifest.id, "org.hito.jamendo");
    assert!(manifest.capabilities.contains(&Capability::Search));
    assert!(manifest.capabilities.contains(&Capability::Sources));
    assert_eq!(provider.api_requests(), 0, "manifest contacted Jamendo");
    assert_eq!(provider.media_requests(), 0, "manifest fetched media");

    let response = client
        .post(plugin.base.join("v1/search").unwrap())
        .header("authorization", "Bearer private-hito-token")
        .header("cookie", "private-hito-cookie")
        .json(&json!({"query": fixture::QUERY, "cursor": null, "limit": 10}))
        .send()
        .await
        .expect("search request failed");
    assert_eq!(response.status(), reqwest::StatusCode::OK);
    let search_bytes = response.bytes().await.expect("search body failed");
    fixture::assert_private_urls_absent(&search_bytes);
    let page: SearchPage = serde_json::from_slice(&search_bytes).expect("invalid search JSON");
    page.validate(10).expect("invalid search contract");
    assert_eq!(page.items.len(), 1);
    assert!(page.next_cursor.is_none());
    let found = &page.items[0];
    assert_eq!(found.title, fixture::TITLE);
    assert_eq!(found.artist, fixture::ARTIST);
    assert_eq!(found.duration_ms, Some(1_000));
    assert_eq!(found.track_number, Some(3));
    let attribution = found
        .attribution
        .as_ref()
        .expect("provider recording credits were lost");
    assert_eq!(attribution.creators, vec![fixture::ARTIST.to_owned()]);
    assert_eq!(attribution.provider_label, "Jamendo");
    assert_eq!(attribution.content_url, fixture::CONTENT_URL);
    let release = found.release.as_ref().expect("provider album was lost");
    assert_eq!(release.title, fixture::ALBUM);
    assert_eq!(release.artist.as_deref(), Some(fixture::ARTIST));
    assert_eq!(
        provider.api_requests(),
        1,
        "search did not use the read API"
    );
    assert_eq!(provider.media_requests(), 0, "search prefetched media");

    let selection = json!({"recording_id": found.id});
    let response = client
        .post(plugin.base.join("v1/recordings/detail").unwrap())
        .json(&selection)
        .send()
        .await
        .expect("detail request failed");
    assert_eq!(response.status(), reqwest::StatusCode::OK);
    let detail_bytes = response.bytes().await.expect("detail body failed");
    fixture::assert_private_urls_absent(&detail_bytes);
    let detail: ProviderRecording =
        serde_json::from_slice(&detail_bytes).expect("invalid detail JSON");
    detail.validate().expect("invalid detail contract");
    assert!(detail == *found, "detail changed the recording projection");

    let response = client
        .post(plugin.base.join("v1/sources").unwrap())
        .json(&selection)
        .send()
        .await
        .expect("source request failed");
    assert_eq!(response.status(), reqwest::StatusCode::OK);
    let source_bytes = response.bytes().await.expect("source body failed");
    fixture::assert_private_urls_absent(&source_bytes);
    let sources: SourcePage = serde_json::from_slice(&source_bytes).expect("invalid source JSON");
    sources.validate().expect("invalid source contract");
    assert_eq!(sources.items.len(), 1);
    assert!(sources.items[0].ready);
    assert_eq!(provider.media_requests(), 0, "selection fetched media");

    let mut audio_url = plugin.base.join("v1/audio").unwrap();
    audio_url
        .query_pairs_mut()
        .append_pair("source_id", &sources.items[0].id);
    let response = client
        .get(audio_url)
        .header("authorization", "Bearer private-hito-token")
        .header("cookie", "private-hito-cookie")
        .send()
        .await
        .expect("audio request failed");
    assert_eq!(response.status(), reqwest::StatusCode::OK);
    assert_eq!(response.headers()["content-type"], "audio/wav");
    assert_eq!(response.headers()["cache-control"], "private, no-store");
    assert!(response.headers().get("set-cookie").is_none());
    assert!(response.headers().get("location").is_none());
    let audio = response.bytes().await.expect("audio body failed");
    assert_eq!(audio.as_ref(), provider.wav.as_slice());
    assert_eq!(provider.media_requests(), 1);
    assert_eq!(
        provider.download_requests(),
        0,
        "stream used a download URL"
    );
    assert_eq!(provider.artwork_requests(), 0, "stream prefetched artwork");
    assert!(
        !provider.rejected_request(),
        "upstream protocol or privacy failed"
    );
}

#[tokio::test]
async fn selected_source_refetches_authority_before_rejecting_a_changed_foreign_stream() {
    let provider = Provider::start().await;
    let foreign = Provider::start().await;
    let capture = logging::Capture::default();
    let config = Config::fixture(CLIENT_ID.to_owned(), provider.base.clone());
    let plugin = Server::start_with_dispatch(
        router(config).expect("Jamendo router setup failed"),
        tracing::Dispatch::new(capture.clone()),
    )
    .await;
    let client = reqwest::Client::builder()
        .no_proxy()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(Duration::from_secs(3))
        .build()
        .expect("Jamendo fixture client setup failed");
    let response = client
        .post(plugin.base.join("v1/search").unwrap())
        .json(&json!({"query": fixture::QUERY, "cursor": null, "limit": 10}))
        .send()
        .await
        .expect("source authority search failed");
    assert_eq!(response.status(), reqwest::StatusCode::OK);
    let search_bytes = response
        .bytes()
        .await
        .expect("source authority search body failed");
    fixture::assert_private_urls_absent(&search_bytes);
    let page: SearchPage = serde_json::from_slice(&search_bytes).expect("invalid search JSON");
    page.validate(10).expect("invalid search contract");
    assert_eq!(page.items.len(), 1);
    let response = client
        .post(plugin.base.join("v1/sources").unwrap())
        .json(&json!({"recording_id": page.items[0].id}))
        .send()
        .await
        .expect("source authority selection failed");
    assert_eq!(response.status(), reqwest::StatusCode::OK);
    let source_bytes = response
        .bytes()
        .await
        .expect("source authority selection body failed");
    fixture::assert_private_urls_absent(&source_bytes);
    let sources: SourcePage = serde_json::from_slice(&source_bytes).expect("invalid source JSON");
    sources.validate().expect("invalid source contract");
    assert_eq!(sources.items.len(), 1);
    assert!(sources.items[0].ready);
    assert_eq!(provider.media_requests(), 0);
    let before_audio = provider.api_requests();

    provider.replace_audio_url(
        foreign
            .base
            .join("audio/owned.wav?private=changed-foreign-stream")
            .unwrap(),
    );
    let mut endpoint = plugin.base.join("v1/audio").unwrap();
    endpoint
        .query_pairs_mut()
        .append_pair("source_id", &sources.items[0].id);
    let response = client
        .get(endpoint)
        .header("authorization", "Bearer private-authority-hito-token")
        .header("cookie", "private-authority-hito-cookie")
        .send()
        .await
        .expect("changed source audio request failed");
    assert_eq!(response.status(), reqwest::StatusCode::BAD_GATEWAY);
    assert_eq!(response.headers()["cache-control"], "private, no-store");
    assert!(response.headers().get("location").is_none());
    assert!(response.headers().get("set-cookie").is_none());
    let body = response
        .bytes()
        .await
        .expect("changed source response body failed");
    fixture::assert_private_urls_absent(&body);
    assert!(
        provider.api_requests() > before_audio,
        "audio reused stale authority"
    );
    assert_eq!(
        provider.media_requests(),
        0,
        "audio opened the previous stream"
    );
    assert_eq!(foreign.requests(), 0, "audio contacted an untrusted origin");
    assert!(
        !provider.rejected_request(),
        "authoritative request leaked caller credentials"
    );

    let logs = capture.text();
    for forbidden in [
        CLIENT_ID,
        fixture::QUERY,
        fixture::TITLE,
        fixture::ARTIST,
        fixture::ALBUM,
        "private-",
        "http://",
        "https://",
        "127.0.0.1",
        sources.items[0].id.as_str(),
    ] {
        assert!(
            !logs.contains(forbidden),
            "source authority logs exposed private data"
        );
    }
    assert!(
        logs.contains("jamendo_provider_complete"),
        "provider log capture was inactive"
    );
    assert!(
        logs.contains("invalid_response"),
        "safe rejection outcome was not logged"
    );
    assert!(
        logs.contains("request_id="),
        "request correlation was not logged"
    );
}
