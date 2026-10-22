use crate::{
    assert_private_logs,
    fixture::{self, Library},
    logs, provider,
};
use axum::http::StatusCode;
use futures_util::FutureExt;
use hito_plugin_sdk::LyricsRequest;
use serde_json::json;
use sha2::{Digest, Sha256};
use std::{
    panic::{AssertUnwindSafe, resume_unwind},
    time::Duration,
};
use tracing::instrument::WithSubscriber;

const CHANGED_TEXT: &str = "Owned changed private lyric text canary";

#[tokio::test]
async fn validated_document_digests_are_stable_and_distinguish_same_revision_changes() {
    let provider = provider::Provider::start().await;
    let result = tokio::time::timeout(
        Duration::from_secs(30),
        AssertUnwindSafe(digests(&provider)).catch_unwind(),
    )
    .await;
    provider.finish().await;
    match result {
        Ok(Ok(())) => {}
        Ok(Err(panic)) => resume_unwind(panic),
        Err(_) => panic!("The owned lyric digest workflow must finish within its deadline"),
    }
}

async fn digests(provider: &provider::Provider) {
    let library = Library::prepare(provider).await;
    let before = library.snapshot().await;
    let capture = logs::Logs::default();
    let subscriber = tracing::Dispatch::new(capture.subscriber());
    let mut anonymous = library.account.headers();
    anonymous.remove("cookie");
    let mut stale = library.lookup_body();
    stale["installation_revision"] = json!(library.installation_revision + 1);
    for (headers, body, expected) in [
        (anonymous, library.lookup_body(), StatusCode::UNAUTHORIZED),
        (library.account.headers(), stale, StatusCode::CONFLICT),
    ] {
        let (status, headers, result) =
            fixture::exchange(&library.app, "POST", &library.lookup_path(), body, headers)
                .with_subscriber(subscriber.clone())
                .await;
        assert_eq!(status, expected);
        assert!(result.get("document").is_none() && result.get("document_digest").is_none());
        if expected == StatusCode::CONFLICT {
            assert!(headers["cache-control"] == "private, no-store");
        }
        provider.assert_lookups(0).await;
        assert!(library.snapshot().await == before);
    }

    let original = provider::response(true);
    let mut timing = original.clone();
    timing.lyrics.as_mut().unwrap().lines[0].end_ms = 1500;
    let mut text = original.clone();
    text.lyrics.as_mut().unwrap().lines[0].text = CHANGED_TEXT.into();
    let documents = [original.clone(), original, timing, text];
    let expected_digests: Vec<String> = documents
        .iter()
        .map(|document| {
            format!(
                "{:x}",
                Sha256::digest(serde_json::to_vec(document).unwrap())
            )
        })
        .collect();
    let mut results = Vec::new();
    for (index, document) in documents.iter().enumerate() {
        assert!(
            document
                .validate_for(&LyricsRequest {
                    recording: provider::reference()
                })
                .is_ok()
        );
        let lyrics = document.lyrics.as_ref().unwrap();
        let original = documents[0].lyrics.as_ref().unwrap();
        assert!(
            lyrics.id == original.id
                && lyrics.rendition_id == original.rendition_id
                && lyrics.revision == original.revision
        );
        provider.lyrics(json!(document)).await;
        let (status, headers, result) = fixture::exchange(
            &library.app,
            "POST",
            &library.lookup_path(),
            library.lookup_body(),
            library.account.headers(),
        )
        .with_subscriber(subscriber.clone())
        .await;
        assert_eq!(status, StatusCode::OK);
        assert!(headers["cache-control"] == "private, no-store");
        assert!(
            result["status"] == "ready"
                && result["recording_id"] == library.recording
                && result["observation_id"] == library.observation
                && result["document"] == json!(document)
        );
        provider.assert_lookups(index + 1).await;
        assert!(
            library.snapshot().await == before,
            "Reading document digests must preserve catalog, private collection and plugin state"
        );
        library.assert_original();
        results.push(result);
    }
    let mut private: Vec<&str> = expected_digests.iter().map(String::as_str).collect();
    private.push(CHANGED_TEXT);
    assert_private_logs(
        &capture,
        &library,
        provider,
        &["conflict", "ready", "ready", "ready", "ready"],
        &private,
    );

    let digests: Vec<&str> = results
        .iter()
        .map(|result| {
            result["document_digest"]
                .as_str()
                .expect("Authenticated ready lyrics must include a host document digest")
        })
        .collect();
    for (digest, expected) in digests.iter().zip(&expected_digests) {
        assert!(
            *digest == expected.as_str(),
            "The host digest must be the SHA-256 of the complete validated typed response"
        );
    }
    assert!(
        digests[0] == digests[1],
        "Identical documents need a stable digest"
    );
    assert!(
        digests[0] != digests[2] && digests[0] != digests[3],
        "Timing and text changes must each change the digest despite unchanged document identities"
    );
}
