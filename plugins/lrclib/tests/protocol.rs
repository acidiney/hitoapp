mod support;

use hito_plugin_lrclib::{Config, router};
use serde_json::{Value, json};
use support::{Server, TOKEN, Upstream, authorized, record};
use url::Url;

#[tokio::test]
async fn search_returns_lrclib_identities_without_audio_or_lyric_text() {
    let (upstream, upstream_server) = Upstream::start(json!([record(41)])).await;
    let config = Config::new(TOKEN)
        .unwrap()
        .with_endpoint(Url::parse(&upstream_server.url).unwrap())
        .unwrap();
    let service = Server::start(router(config).unwrap()).await;
    let client = reqwest::Client::new();
    let manifest: Value = client
        .get(format!("{}v1/manifest", service.url))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(manifest["id"], "org.hito.lrclib");
    assert_eq!(
        manifest["capabilities"],
        json!(["search", "metadata", "lyrics"])
    );
    assert!(upstream.seen.lock().unwrap().is_empty());
    let response = authorized(client.post(format!("{}v1/search", service.url)))
        .json(&json!({"query": "Owned river Fixture ensemble", "cursor": null, "limit": 5}))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 200);
    assert_eq!(response.headers()["cache-control"], "private, no-store");
    let page: Value = response.json().await.unwrap();
    assert_eq!(page["items"][0]["id"], "41");
    assert_eq!(page["items"][0]["title"], "Owned river");
    assert_eq!(page["items"][0]["artist"], "Fixture ensemble");
    assert_eq!(page["items"][0]["duration_ms"], 123450);
    assert_eq!(
        page["items"][0]["release"],
        Value::Null,
        "LRCLIB does not supply an album identity"
    );
    assert_eq!(page["next_cursor"], Value::Null);
    let encoded = page.to_string();
    assert!(!encoded.contains("Synthetic river"));
    assert!(!encoded.contains("Owned horizon"));
    let calls = upstream.seen.lock().unwrap().clone();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].0, "GET");
    let uri = Url::parse(&format!("http://fixture{}", calls[0].1)).unwrap();
    assert_eq!(uri.path(), "/api/search");
    assert_eq!(
        uri.query_pairs().collect::<Vec<_>>(),
        vec![("q".into(), "Owned river Fixture ensemble".into())]
    );
    assert!(calls[0].2.contains("HitoLRCLIB/"));
    assert!(calls[0].2.contains("contact@example.invalid"));
    assert_eq!(
        authorized(client.post(format!("{}v1/sources", service.url)))
            .json(&json!({"recording_id": "41"}))
            .send()
            .await
            .unwrap()
            .status(),
        404
    );
    assert_eq!(upstream.seen.lock().unwrap().len(), 1);
    service.close().await;
    upstream_server.close().await;
}

#[tokio::test]
async fn metadata_reads_the_exact_lrclib_identity_and_keeps_album_text_separate() {
    let (upstream, upstream_server) = Upstream::start(record(41)).await;
    let config = Config::new(TOKEN)
        .unwrap()
        .with_endpoint(Url::parse(&upstream_server.url).unwrap())
        .unwrap();
    let service = Server::start(router(config).unwrap()).await;
    let client = reqwest::Client::new();
    let response = authorized(client.post(format!("{}v1/recordings/metadata", service.url)))
        .json(&json!({"recording_id": "41"}))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 200);
    let metadata: Value = response.json().await.unwrap();
    assert_eq!(metadata["recording"]["id"], "41");
    assert_eq!(metadata["album_title"], "Owned collection");
    assert_eq!(metadata["recording"]["release"], Value::Null);
    assert_eq!(metadata["recording"]["revision"], Value::Null);
    assert_eq!(metadata["recording"]["isrc"], Value::Null);
    assert_eq!(metadata["source_ids"], Value::Null);
    let calls = upstream.seen.lock().unwrap().clone();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].1, "/api/get/41");
    service.close().await;
    upstream_server.close().await;
}

#[tokio::test]
async fn plain_lyrics_use_version_two_without_invented_timestamps() {
    let mut value = record(41);
    value["syncedLyrics"] = Value::Null;
    let (upstream, upstream_server) = Upstream::start(value).await;
    let config = Config::new(TOKEN)
        .unwrap()
        .with_endpoint(Url::parse(&upstream_server.url).unwrap())
        .unwrap();
    let service = Server::start(router(config).unwrap()).await;
    let client = reqwest::Client::new();
    let response = authorized(client.post(format!("{}v2/lyrics", service.url)))
        .json(&json!({"recording": {"id": "41", "revision": null}}))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 200);
    let document: Value = response.json().await.unwrap();
    assert_eq!(document["recording"], json!({"id": "41", "revision": null}));
    assert_eq!(
        document["lyrics"]["content"],
        json!({"kind": "plain", "lines": ["Synthetic river", "Owned horizon"]})
    );
    assert_eq!(document["lyrics"]["language"], Value::Null);
    assert_eq!(document["lyrics"]["revision"], Value::Null);
    assert_eq!(
        document["lyrics"]["attribution"],
        json!({"provider_label": "LRCLIB", "content_url": "https://lrclib.net/api/get/41"})
    );
    let descriptor: Value =
        authorized(client.get(format!("{}v2/lyrics/capabilities", service.url)))
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
    assert_eq!(descriptor, json!({"lyrics_version": 2}));
    assert_eq!(
        authorized(client.get(format!("{}v1/lyrics/capabilities", service.url)))
            .send()
            .await
            .unwrap()
            .status(),
        404
    );
    let calls = upstream.seen.lock().unwrap().clone();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].1, "/api/get/41");
    service.close().await;
    upstream_server.close().await;
}

#[tokio::test]
async fn lrc_keeps_real_anchors_parallel_lines_and_clear_events() {
    let mut value = record(41);
    value["syncedLyrics"] = json!(
        "[ar:Unverified metadata]\n[00:03.00]Late owned line\n[00:01.00][00:02.00]Owned repeat\n[00:02.00]Parallel owned line\n[00:02.00]Parallel owned line\n[00:04.00]\n"
    );
    let (_, upstream_server) = Upstream::start(value).await;
    let config = Config::new(TOKEN)
        .unwrap()
        .with_endpoint(Url::parse(&upstream_server.url).unwrap())
        .unwrap();
    let service = Server::start(router(config).unwrap()).await;
    let response = authorized(reqwest::Client::new().post(format!("{}v2/lyrics", service.url)))
        .json(&json!({"recording": {"id": "41", "revision": null}}))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 200);
    let document: Value = response.json().await.unwrap();
    assert_eq!(
        document["lyrics"]["content"],
        json!({"kind":"anchored", "lines":[
            {"start_ms":1000,"text":"Owned repeat"},
            {"start_ms":2000,"text":"Owned repeat"},
            {"start_ms":2000,"text":"Parallel owned line"},
            {"start_ms":3000,"text":"Late owned line"},
            {"start_ms":4000,"text":""}
        ]})
    );
    assert_eq!(document["lyrics"]["language"], Value::Null);
    assert!(!document.to_string().contains("end_ms"));
    service.close().await;
    upstream_server.close().await;
}

#[tokio::test]
async fn malformed_or_oversized_timing_uses_owned_plain_text_and_unknown_duration_stays_unknown() {
    let (upstream, upstream_server) = Upstream::start(record(41)).await;
    let config = Config::new(TOKEN)
        .unwrap()
        .with_endpoint(Url::parse(&upstream_server.url).unwrap())
        .unwrap();
    let service = Server::start(router(config).unwrap()).await;
    let client = reqwest::Client::new();
    for synced in [
        "[offset:+250]\n[00:01]Owned".into(),
        format!("{}{}", "[00:01]".repeat(400), "x".repeat(800)),
        format!("[00:01]Owned{}", " ".repeat(2049)),
        "[00:01.1234]Owned".into(),
        "[00:60.00]Owned".into(),
        "[00:01]Owned\n[offset:-2000]".into(),
        "[offset:1]\n[offset:2]\n[00:01]Owned".into(),
        "[03:00]Owned".into(),
        "Owned without anchor".into(),
    ] {
        let mut value = record(41);
        value["syncedLyrics"] = json!(synced);
        *upstream.reply.lock().unwrap() = value;
        let response = authorized(client.post(format!("{}v2/lyrics", service.url)))
            .json(&json!({"recording":{"id":"41","revision":null}}))
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), 200);
        let document: Value = response.json().await.unwrap();
        assert_eq!(
            document["lyrics"]["content"],
            json!({"kind":"plain","lines":["Synthetic river","Owned horizon"]})
        );
    }
    let mut value = record(41);
    value["duration"] = Value::Null;
    value["syncedLyrics"] = json!("[00:01.2]Owned\n[00:02.345]");
    value["plainLyrics"] = Value::Null;
    *upstream.reply.lock().unwrap() = value;
    let document: Value = authorized(client.post(format!("{}v2/lyrics", service.url)))
        .json(&json!({"recording":{"id":"41","revision":null}}))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(document["lyrics"]["duration_ms"], Value::Null);
    assert_eq!(
        document["lyrics"]["content"],
        json!({"kind":"anchored","lines":[{"start_ms":1200,"text":"Owned"},{"start_ms":2345,"text":""}]})
    );
    service.close().await;
    upstream_server.close().await;
}
