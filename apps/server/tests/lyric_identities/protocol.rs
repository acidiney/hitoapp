use super::setup::Prepared;
use crate::lyric_provider;
use hito_plugin_sdk::LyricsResponseV2;
use serde_json::json;
use sha2::{Digest, Sha256};

#[tokio::test]
async fn selected_reads_preserve_plain_anchors_and_null_and_negotiate_a_real_v1_fallback() {
    let prepared = Prepared::new().await;
    let before = prepared.library.snapshot().await;
    for content in [
        json!({"kind":"plain","lines":["Owned plain line",""]}),
        json!({"kind":"anchored","lines":[{"start_ms":0,"text":"Owned anchored line"},{"start_ms":0,"text":"Owned parallel line"},{"start_ms":900,"text":""}]}),
    ] {
        prepared
            .provider
            .lyrics(lyric_provider::response(content.clone()))
            .await;
        let result = prepared.read().await;
        assert_eq!(result["status"], "ready_v2");
        assert_eq!(result["document"]["lyrics"]["content"], content);
        let document: LyricsResponseV2 =
            serde_json::from_value(result["document"].clone()).unwrap();
        let digest = format!(
            "{:x}",
            Sha256::digest(serde_json::to_vec(&document).unwrap())
        );
        assert_eq!(result["document_digest"], digest);
        assert_eq!(result["provider_recording"], lyric_provider::reference());
    }
    prepared
        .provider
        .lyrics(json!({"recording":lyric_provider::reference(),"lyrics":null}))
        .await;
    let result = prepared.read().await;
    assert_eq!(result["status"], "ready_v2");
    assert!(result["document"]["lyrics"].is_null());
    prepared.provider.descriptor(None).await;
    let result = prepared.read().await;
    assert_eq!(result["status"], "ready_v1");
    assert_eq!(
        result["document"],
        json!({"recording":lyric_provider::reference(),"lyrics":null})
    );
    assert_eq!(
        prepared.provider.calls(),
        vec![
            "manifest",
            "metadata",
            "metadata",
            "v2_descriptor",
            "v2_lyrics",
            "v2_descriptor",
            "v2_lyrics",
            "v2_descriptor",
            "v2_lyrics",
            "v2_missing",
            "v1_descriptor",
            "v1_lyrics"
        ]
    );
    assert_eq!(prepared.library.snapshot().await, before);
    prepared.library.assert_original();
    prepared.provider.finish().await;
}

#[tokio::test]
async fn malformed_unrelated_and_oversized_lyrics_are_unavailable_instead_of_negative_answers() {
    let prepared = Prepared::new().await;
    let before = prepared.library.snapshot().await;
    let mut unrelated =
        lyric_provider::response(json!({"kind":"plain","lines":["Owned unrelated line"]}));
    unrelated["recording"]["id"] = json!("other-provider-recording");
    let mut invalid_anchors = lyric_provider::response(
        json!({"kind":"anchored","lines":[{"start_ms":900,"text":"Later"},{"start_ms":0,"text":"Earlier"}]}),
    );
    invalid_anchors["lyrics"]["duration_ms"] = json!(1000);
    for value in [
        unrelated,
        invalid_anchors,
        json!({"recording":lyric_provider::reference(),"lyrics":{"content":{"kind":"plain","lines":["Missing identity"]}}}),
    ] {
        prepared.provider.lyrics(value).await;
        assert_eq!(prepared.read().await, json!({"status":"unavailable"}));
    }
    prepared.provider.lyrics_bytes(b"{not json".to_vec()).await;
    assert_eq!(prepared.read().await, json!({"status":"unavailable"}));
    let mut oversized = vec![b' '; 262144];
    oversized.extend(
        serde_json::to_vec(&json!({"recording":lyric_provider::reference(),"lyrics":null}))
            .unwrap(),
    );
    prepared.provider.lyrics_bytes(oversized).await;
    assert_eq!(prepared.read().await, json!({"status":"unavailable"}));
    prepared
        .provider
        .descriptor(Some(
            json!({"lyrics_version":2,"untrusted":"x".repeat(5000)}),
        ))
        .await;
    assert_eq!(prepared.read().await, json!({"status":"unavailable"}));
    let expected = vec![
        "manifest",
        "metadata",
        "metadata",
        "v2_descriptor",
        "v2_lyrics",
        "v2_descriptor",
        "v2_lyrics",
        "v2_descriptor",
        "v2_lyrics",
        "v2_descriptor",
        "v2_lyrics",
        "v2_descriptor",
        "v2_lyrics",
        "v2_descriptor",
    ];
    assert_eq!(prepared.provider.calls(), expected);
    assert_eq!(prepared.library.snapshot().await, before);
    prepared.library.assert_original();
    prepared.provider.finish().await;
}
