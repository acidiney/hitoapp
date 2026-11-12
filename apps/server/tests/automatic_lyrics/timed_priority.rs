use super::setup::Prepared;
use crate::{account::Account, provider};
use axum::http::StatusCode;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

pub(super) fn candidate(provider: &provider::Provider, id: &str, content: Option<Value>) -> Value {
    let mut recording = provider::recording();
    recording["id"] = json!(id);
    let reference = json!({"id":id,"revision":"owned:1"});
    let mut document = provider::response();
    document["recording"] = reference.clone();
    match content {
        Some(content) => {
            document["lyrics"]["id"] = json!(format!("owned:document:{id}"));
            document["lyrics"]["rendition_id"] = json!(format!("owned:rendition:{id}"));
            document["lyrics"]["content"] = content;
        }
        None => document["lyrics"] = Value::Null,
    }
    provider.reply_for(
        "metadata",
        json!({"recording_id":id}),
        json!({"recording":recording}),
    );
    provider.reply_for("v2_lyrics", json!({"recording":reference}), document);
    recording
}

pub(super) fn content(timed: bool) -> Value {
    if timed {
        json!({"kind":"anchored","lines":[{"start_ms":123,"text":"Owned timed content"}]})
    } else {
        json!({"kind":"plain","lines":["Owned plain content"]})
    }
}

#[tokio::test]
async fn automatic_read_prefers_a_later_valid_timed_identity_over_a_compatible_plain_identity() {
    let prepared = Prepared::with_duration(120).await;
    let library = &prepared.library;
    let mut plain = provider::recording();
    plain["duration_ms"] = json!(120_000);
    let mut timed = plain.clone();
    timed["id"] = json!("owned:timed-second");
    let timed_reference = json!({"id":"owned:timed-second","revision":"owned:1"});
    let mut timed_document = provider::response();
    timed_document["recording"] = timed_reference.clone();
    timed_document["lyrics"]["id"] = json!("owned:timed-second-document");
    timed_document["lyrics"]["rendition_id"] = json!("owned:timed-second-rendition");
    timed_document["lyrics"]["duration_ms"] = json!(120_000);
    timed_document["lyrics"]["content"] = json!({"kind":"anchored","lines":[
        {"start_ms":0,"text":"Owned timed first line"},
        {"start_ms":60_000,"text":"Owned timed later line"},
    ]});
    let parsed: hito_plugin_sdk::LyricsResponseV2 =
        serde_json::from_value(timed_document.clone()).unwrap();
    let expected_digest = format!("{:x}", Sha256::digest(serde_json::to_vec(&parsed).unwrap()));
    prepared.provider.reply(
        "search",
        json!({"items":[plain.clone(),timed.clone()],"next_cursor":null}),
    );
    for candidate in [plain, timed] {
        prepared.provider.reply_for(
            "metadata",
            json!({"recording_id":candidate["id"]}),
            json!({"recording":candidate}),
        );
    }
    prepared.provider.reply_for(
        "v2_lyrics",
        json!({"recording":provider::reference()}),
        provider::response(),
    );
    prepared.provider.reply_for(
        "v2_lyrics",
        json!({"recording":timed_reference}),
        timed_document.clone(),
    );
    let (status, user) = library
        .account
        .request(
            &library.app,
            "POST",
            "/api/v1/users",
            json!({"username":"timed-priority-peer","password":"owned timed priority peer password"}),
        )
        .await;
    assert_eq!(status, StatusCode::CREATED);
    let peer = Account::login(
        &library.app,
        "timed-priority-peer",
        "owned timed priority peer password",
    )
    .await;
    let libraries = library.account.get(&library.app, "/api/v1/libraries").await;
    let shared = libraries
        .as_array()
        .unwrap()
        .iter()
        .find(|item| item["personal"] == false)
        .unwrap()["id"]
        .as_str()
        .unwrap();
    let (status, _) = library
        .account
        .request(
            &library.app,
            "PUT",
            &format!(
                "/api/v1/libraries/{shared}/members/{}",
                user["id"].as_str().unwrap()
            ),
            Value::Null,
        )
        .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let identities_path = prepared.path().replace("/automatic", "/identities");
    let before = library.snapshot().await;
    let peer_before = peer.get(&library.app, "/api/v1/recordings").await;
    assert_eq!(
        library.account.get(&library.app, &identities_path).await["items"],
        json!([])
    );

    let response = prepared.read().await;
    assert_eq!(library.snapshot().await, before);
    assert_eq!(
        library.account.get(&library.app, &identities_path).await["items"],
        json!([])
    );
    let calls_before_peer = prepared.provider.calls();
    let (status, peer_response) = peer
        .request(&library.app, "POST", &prepared.path(), json!({}))
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(peer_response, json!({"status":"no_services"}));
    assert_eq!(prepared.provider.calls(), calls_before_peer);
    assert_eq!(
        peer.get(&library.app, "/api/v1/recordings").await,
        peer_before
    );
    assert_eq!(
        peer.get(&library.app, &identities_path).await["items"],
        json!([])
    );
    let metadata_requests = prepared.provider.requests_for("metadata");
    let lyric_requests = prepared.provider.requests_for("v2_lyrics");
    let installation = prepared.installation.clone();
    let recording = library.recording.clone();
    prepared.finish().await;

    assert_eq!(response["status"], "ready_v2");
    assert_eq!(response["recording_id"], recording);
    assert_eq!(response["installation_id"], installation["id"]);
    assert_eq!(response["installation_revision"], installation["revision"]);
    assert_eq!(response["provider_recording"], timed_reference);
    assert_eq!(response["document"], serde_json::to_value(parsed).unwrap());
    assert_eq!(response["document_digest"], expected_digest);
    assert!(response.get("selector").is_none());
    assert_eq!(
        metadata_requests,
        vec![
            json!({"recording_id":"owned:automatic"}),
            json!({"recording_id":"owned:timed-second"}),
        ]
    );
    assert_eq!(
        lyric_requests,
        vec![
            json!({"recording":provider::reference()}),
            json!({"recording":timed_reference}),
        ]
    );
    assert_eq!(
        calls_before_peer
            .iter()
            .filter(|operation| **operation == "v2_descriptor")
            .count(),
        1,
        "One admitted scan must negotiate its lyric protocol once"
    );
}
