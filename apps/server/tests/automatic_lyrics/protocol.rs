use super::setup::Prepared;
use crate::provider;
use axum::http::StatusCode;
use serde_json::json;
use sha2::{Digest, Sha256};

#[tokio::test]
async fn automatic_read_preserves_valid_v2_anchors_and_negotiates_legacy_v1_without_fabricated_identity()
 {
    let prepared = Prepared::new().await;
    let before = prepared.library.snapshot().await;
    let content = json!({"kind":"anchored","lines":[
        {"start_ms":123,"text":"Owned parallel first"},
        {"start_ms":123,"text":"Owned parallel second"},
        {"start_ms":999,"text":""},
    ]});
    let mut document = provider::response();
    document["lyrics"]["content"] = content.clone();
    prepared.provider.reply("v2_lyrics", document.clone());
    let parsed: hito_plugin_sdk::LyricsResponseV2 = serde_json::from_value(document).unwrap();
    let ready = prepared.read().await;
    assert_eq!(ready["status"], "ready_v2");
    assert_eq!(ready["document"]["lyrics"]["content"], content);
    assert_eq!(
        ready["document_digest"],
        format!("{:x}", Sha256::digest(serde_json::to_vec(&parsed).unwrap()))
    );
    assert!(ready.get("selector").is_none());
    let lines = json!([{"start_ms":0,"end_ms":1000,"text":"Owned legacy interval"}]);
    prepared.provider.reply(
        "v1_lyrics",
        json!({"recording":provider::reference(),"lyrics":{
            "id":"owned:legacy-doc","rendition_id":"owned:legacy-rendition","lines":lines,
        }}),
    );
    for status in [StatusCode::NOT_FOUND, StatusCode::NOT_IMPLEMENTED] {
        prepared.provider.status("v2_descriptor", status);
        let ready = prepared.read().await;
        assert_eq!(ready["status"], "ready_v1");
        assert_eq!(ready["document"]["lyrics"]["lines"], lines);
        assert_eq!(ready["provider_recording"], provider::reference());
        assert!(ready.get("selector").is_none());
    }
    prepared.provider.status("v2_descriptor", StatusCode::OK);
    prepared
        .provider
        .reply("v2_descriptor", json!({"lyrics_version":3}));
    assert_eq!(prepared.read().await["status"], "ready_v1");
    prepared
        .provider
        .reply("v2_descriptor", json!({"lyrics_version":2}));
    prepared.provider.reply(
        "v2_lyrics",
        json!({"recording":provider::reference(),"lyrics":null}),
    );
    assert_eq!(prepared.read().await, json!({"status":"not_found"}));
    prepared
        .provider
        .reply("v2_descriptor", json!({"lyrics_version":3}));
    prepared
        .provider
        .reply("v1_descriptor", json!({"lyrics_version":2}));
    assert_eq!(prepared.read().await, json!({"status":"unsupported"}));
    assert_eq!(prepared.library.snapshot().await, before);
    prepared.finish().await;
}

#[tokio::test]
async fn malformed_oversized_or_wrong_identity_upstream_results_are_unavailable_instead_of_missing_lyrics()
 {
    let prepared = Prepared::new().await;
    let before = prepared.library.snapshot().await;
    let search = json!({"items":[provider::recording()],"next_cursor":null});
    let metadata = json!({"recording":provider::recording()});
    let descriptor = json!({"lyrics_version":2});
    let document = provider::response();
    let mut wrong_recording = provider::recording();
    wrong_recording["id"] = json!("owned:wrong-recording");
    let mut wrong_revision = provider::recording();
    wrong_revision["revision"] = json!("owned:changed-revision");
    let mut wrong_document = document.clone();
    wrong_document["recording"]["id"] = json!("owned:wrong-document-identity");
    let mut invalid_anchors = document.clone();
    invalid_anchors["lyrics"]["content"] = json!({"kind":"anchored","lines":[{"start_ms":999,"text":"Owned later"},{"start_ms":1,"text":"Owned earlier"}]});
    for (operation, invalid, valid) in [
        (
            "search",
            json!({"items":vec![provider::recording();21],"next_cursor":null}),
            search.clone(),
        ),
        (
            "metadata",
            json!({"recording":wrong_recording}),
            metadata.clone(),
        ),
        (
            "metadata",
            json!({"recording":wrong_revision}),
            metadata.clone(),
        ),
        ("v2_lyrics", wrong_document, document.clone()),
        ("v2_lyrics", invalid_anchors, document.clone()),
    ] {
        prepared.provider.reply(operation, invalid);
        let response = prepared.read().await;
        assert_eq!(response["status"], "unavailable");
        assert!(response.get("document").is_none());
        prepared.provider.reply(operation, valid);
    }
    for (operation, valid) in [
        ("search", search),
        ("metadata", metadata),
        ("v2_descriptor", descriptor),
        ("v2_lyrics", document.clone()),
    ] {
        prepared
            .provider
            .bytes(operation, b"owned invalid JSON".to_vec());
        assert_eq!(prepared.read().await["status"], "unavailable");
        prepared.provider.reply(operation, valid);
    }
    prepared.provider.bytes("v2_lyrics", vec![b'x'; 300_000]);
    assert_eq!(prepared.read().await["status"], "unavailable");
    prepared.provider.reply("v2_lyrics", document);
    for status in [
        StatusCode::UNAUTHORIZED,
        StatusCode::FORBIDDEN,
        StatusCode::TOO_MANY_REQUESTS,
        StatusCode::BAD_GATEWAY,
        StatusCode::SERVICE_UNAVAILABLE,
    ] {
        prepared.provider.status("search", status);
        assert_eq!(prepared.read().await["status"], "unavailable");
    }
    prepared.provider.status("search", StatusCode::OK);
    assert_eq!(prepared.read().await["status"], "ready_v2");
    assert_eq!(prepared.library.snapshot().await, before);
    prepared.finish().await;
}

#[tokio::test]
async fn automatic_disclosure_requires_post_valid_empty_json_and_the_existing_account_origin_policy()
 {
    let prepared = Prepared::new().await;
    let (status, _) = prepared
        .library
        .account
        .request(&prepared.library.app, "GET", &prepared.path(), json!(null))
        .await;
    assert_eq!(status, StatusCode::METHOD_NOT_ALLOWED);
    let (status, _) = prepared
        .library
        .account
        .request(
            &prepared.library.app,
            "POST",
            &prepared.path(),
            json!({"query":"Owned caller supplied private query"}),
        )
        .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    for (changed, value, expected) in [
        (
            "origin",
            Some("http://untrusted.invalid"),
            StatusCode::FORBIDDEN,
        ),
        ("cookie", None, StatusCode::UNAUTHORIZED),
        (
            "content-type",
            Some("text/plain"),
            StatusCode::UNSUPPORTED_MEDIA_TYPE,
        ),
    ] {
        let mut headers = prepared.library.account.headers();
        if let Some(value) = value {
            headers.insert(changed, value.parse().unwrap());
        } else {
            headers.remove(changed);
        }
        let (status, _, _) = crate::account::exchange(
            &prepared.library.app,
            "POST",
            &prepared.path(),
            json!({}),
            headers,
        )
        .await;
        assert_eq!(status, expected);
    }
    assert_eq!(prepared.provider.calls(), vec!["manifest"]);
    prepared.finish().await;
}
