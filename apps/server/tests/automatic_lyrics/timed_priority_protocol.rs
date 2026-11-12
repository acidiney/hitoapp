use super::{
    setup::Prepared,
    timed_priority::{candidate, content},
};
use crate::{fixture, lrclib, setup::commit_metadata};
use axum::http::StatusCode;
use serde_json::{Value, json};

#[tokio::test]
async fn automatic_scan_reuses_legacy_negotiation_without_downgrading_invalid_v2_content() {
    let prepared = Prepared::new().await;
    let plain = candidate(&prepared.provider, "owned:plain", Some(content(false)));
    let timed = candidate(
        &prepared.provider,
        "owned:legacy-timed",
        Some(content(true)),
    );
    prepared
        .provider
        .reply("search", json!({"items":[plain,timed],"next_cursor":null}));
    prepared
        .provider
        .status("v2_descriptor", StatusCode::NOT_FOUND);
    prepared.provider.reply_for(
        "v1_lyrics",
        json!({"recording":{"id":"owned:plain","revision":"owned:1"}}),
        json!({"recording":{"id":"owned:plain","revision":"owned:1"},"lyrics":null}),
    );
    let legacy = json!({"recording":{"id":"owned:legacy-timed","revision":"owned:1"},"lyrics":{
        "id":"owned:legacy-document", "rendition_id":"owned:legacy-rendition",
        "lines":[{"start_ms":123,"end_ms":999,"text":"Owned legacy timed line"}],
    }});
    prepared.provider.reply_for(
        "v1_lyrics",
        json!({"recording":{"id":"owned:legacy-timed","revision":"owned:1"}}),
        legacy.clone(),
    );
    let before = prepared.library.snapshot().await;
    let response = prepared.read().await;
    assert_eq!(response["status"], "ready_v1");
    assert_eq!(response["provider_recording"]["id"], "owned:legacy-timed");
    assert_eq!(
        response["document"]["lyrics"]["lines"],
        legacy["lyrics"]["lines"]
    );
    let calls = prepared.provider.calls();
    for operation in ["v2_descriptor", "v1_descriptor"] {
        assert_eq!(calls.iter().filter(|call| **call == operation).count(), 1);
    }
    assert_eq!(prepared.provider.requests_for("v1_lyrics").len(), 2);
    assert!(prepared.provider.requests_for("v2_lyrics").is_empty());
    prepared.provider.status("v2_descriptor", StatusCode::OK);
    prepared.provider.reply_for("v2_lyrics", json!({"recording":{"id":"owned:legacy-timed","revision":"owned:1"}}), json!({"recording":{"id":"owned:legacy-timed","revision":"owned:1"},"lyrics":{
        "id":"owned:invalid-v2", "rendition_id":"owned:invalid-v2-rendition",
        "content":{"kind":"anchored","lines":[{"start_ms":999,"text":"Owned later"},{"start_ms":1,"text":"Owned earlier"}]},
    }}));
    assert_eq!(prepared.read().await, json!({"status":"unavailable"}));
    assert_eq!(prepared.provider.requests_for("v1_lyrics").len(), 2);
    assert_eq!(prepared.library.snapshot().await, before);
    prepared.finish().await;
}

#[tokio::test]
async fn automatic_timed_priority_composes_the_actual_lrclib_router_and_owned_upstream() {
    for variant in [
        "valid_timed",
        "malformed_with_plain",
        "malformed_timing_only",
    ] {
        let mut timed = lrclib::record(4243);
        timed["syncedLyrics"] = if variant == "valid_timed" {
            json!("[00:00.123]Owned composed timed line")
        } else {
            json!("[offset:123]\n[00:00.123]Owned unsupported timing")
        };
        if variant == "malformed_timing_only" {
            timed["plainLyrics"] = Value::Null;
        }
        let upstream = lrclib::Upstream::with_records(vec![lrclib::record(4242), timed]).await;
        let config = hito_plugin_lrclib::Config::new(lrclib::TOKEN)
            .unwrap()
            .with_endpoint(upstream.server.url.parse().unwrap())
            .unwrap();
        let shutdown = config.shutdown_handle();
        let plugin = lrclib::Server::start(hito_plugin_lrclib::router(config).unwrap()).await;
        let library = fixture::Library::prepare().await;
        commit_metadata(&library, lrclib::TITLE, lrclib::ARTIST).await;
        let installation = library
            .account
            .connect(
                &library.app,
                &plugin.url,
                "Owned composed timed lyrics",
                Some(lrclib::TOKEN),
            )
            .await;
        let base = format!("/api/v1/recordings/{}/lyrics", library.recording);
        let before = library.snapshot().await;
        let (status, response) = library
            .account
            .request(
                &library.app,
                "POST",
                &format!("{base}/automatic"),
                json!({}),
            )
            .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(library.snapshot().await, before);
        assert_eq!(
            library
                .account
                .get(&library.app, &format!("{base}/identities"))
                .await["items"],
            json!([])
        );
        library.assert_original();
        upstream.assert_calls(&["search", "record", "record", "record", "record"]);
        upstream.assert_record_ids(&[4242, 4242, 4243, 4243]);
        shutdown.cancel();
        plugin.finish().await;
        upstream.server.finish().await;
        if variant == "malformed_timing_only" {
            assert_eq!(response, json!({"status":"unavailable"}));
        } else {
            assert_eq!(response["status"], "ready_v2");
            assert_eq!(response["installation_id"], installation["id"]);
            assert_eq!(
                response["provider_recording"]["id"],
                if variant == "valid_timed" {
                    "4243"
                } else {
                    "4242"
                }
            );
            assert_eq!(
                response["document"]["lyrics"]["content"]["kind"],
                if variant == "valid_timed" {
                    "anchored"
                } else {
                    "plain"
                }
            );
        }
    }
}
