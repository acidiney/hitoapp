mod support;

use hito_plugin_lrclib::{Config, router};
use serde_json::{Value, json};
use support::{Server, TOKEN, Upstream, authorized, record};
use url::Url;

async fn read(value: Value) -> (u16, Value) {
    let (upstream, upstream_server) = Upstream::start(value).await;
    let config = Config::new(TOKEN)
        .unwrap()
        .with_endpoint(Url::parse(&upstream_server.url).unwrap())
        .unwrap();
    let service = Server::start(router(config).unwrap()).await;
    let response = authorized(reqwest::Client::new().post(format!("{}v2/lyrics", service.url)))
        .json(&json!({"recording":{"id":"41","revision":null}}))
        .send()
        .await
        .unwrap();
    let status = response.status().as_u16();
    let body = response.json().await.unwrap();
    assert_eq!(upstream.seen.lock().unwrap().len(), 1);
    service.close().await;
    upstream_server.close().await;
    (status, body)
}

#[tokio::test]
async fn every_raw_field_is_validated_before_absence_instrumental_or_rendition_selection() {
    let cases = [
        (
            "oversized blank sync",
            false,
            Value::Null,
            json!(" ".repeat(4097)),
        ),
        ("sync control only", false, Value::Null, json!("\t")),
        ("instrumental plain control", true, json!("\t"), Value::Null),
        (
            "plain control behind anchors",
            false,
            json!("\t"),
            json!("[00:01]Owned line"),
        ),
        (
            "sync control behind plain",
            false,
            json!("Owned plain line"),
            json!("\t"),
        ),
        (
            "instrumental oversized blank plain",
            true,
            json!(" ".repeat(2049)),
            Value::Null,
        ),
        (
            "excessive empty sync lines",
            false,
            Value::Null,
            json!("\n".repeat(2001)),
        ),
    ];
    let mut outcomes = Vec::new();
    for (name, instrumental, plain, synced) in cases {
        let mut value = record(41);
        value["instrumental"] = json!(instrumental);
        value["plainLyrics"] = plain;
        value["syncedLyrics"] = synced;
        outcomes.push((name, read(value).await.0));
    }
    assert!(
        outcomes.iter().all(|(_, status)| *status == 502),
        "{outcomes:?}"
    );
    let mut value = record(41);
    value["plainLyrics"] = json!("Owned plain fallback");
    value["syncedLyrics"] = json!("[offset:250]\n[00:01]Owned unsupported timing");
    let (status, response) = read(value).await;
    assert_eq!(status, 200);
    assert_eq!(
        response["lyrics"]["content"],
        json!({"kind":"plain","lines":["Owned plain fallback"]})
    );
    let mut value = record(41);
    value["instrumental"] = json!(true);
    value["plainLyrics"] = Value::Null;
    value["syncedLyrics"] = Value::Null;
    assert_eq!(
        read(value).await,
        (
            200,
            json!({"recording":{"id":"41","revision":null},"lyrics":null})
        )
    );
}

#[tokio::test]
async fn unknown_revisions_still_require_a_positive_canonical_numeric_provider_id() {
    let (upstream, upstream_server) = Upstream::start(record(41)).await;
    let config = Config::new(TOKEN)
        .unwrap()
        .with_endpoint(Url::parse(&upstream_server.url).unwrap())
        .unwrap();
    let service = Server::start(router(config).unwrap()).await;
    let client = reqwest::Client::new();
    let mut outcomes = Vec::new();
    for id in [
        "001",
        "0",
        "../private",
        "18446744073709551616",
        "+41",
        "-1",
    ] {
        let response = authorized(client.post(format!("{}v2/lyrics", service.url)))
            .json(&json!({"recording":{"id":id,"revision":"unknown-owned-revision"}}))
            .send()
            .await
            .unwrap();
        outcomes.push((id, response.status().as_u16()));
    }
    assert!(upstream.seen.lock().unwrap().is_empty());
    service.close().await;
    upstream_server.close().await;
    assert!(
        outcomes.iter().all(|(_, status)| *status == 400),
        "{outcomes:?}"
    );
}
