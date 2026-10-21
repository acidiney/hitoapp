use super::{
    AssertUnwindSafe, Duration, FutureExt, Library, StatusCode, assert_private_logs, fixture, json,
    logs, provider, resume_unwind,
};
use hito_plugin_sdk::LyricsResponse;
use serde_json::Value;
use sha2::{Digest, Sha256};
use tracing::instrument::WithSubscriber;

#[path = "../fixtures/timed_lyrics_attached.rs"]
pub(super) mod attached;

#[tokio::test]
async fn an_account_lists_current_lyric_providers_without_contacting_them_or_changing_its_library()
{
    let provider = provider::Provider::start().await;
    let result = tokio::time::timeout(
        Duration::from_secs(30),
        AssertUnwindSafe(list_current(&provider)).catch_unwind(),
    )
    .await;
    provider.finish().await;
    match result {
        Ok(Ok(())) => {}
        Ok(Err(panic)) => resume_unwind(panic),
        Err(_) => panic!("The owned local lyric-provider read must finish within its deadline"),
    }
}

async fn list_current(provider: &provider::Provider) {
    let library = Library::prepare(provider).await;
    let before = library.snapshot().await;
    provider.assert_lookups(0).await;
    let capture = logs::Logs::default();
    let (status, headers, result) = fixture::exchange(
        &library.app,
        "GET",
        &format!("{}/providers", library.lookup_path()),
        Value::Null,
        library.account.headers(),
    )
    .with_subscriber(capture.subscriber())
    .await;
    assert_eq!(
        status,
        StatusCode::OK,
        "An authenticated owner must read its current approved lyric-provider identities"
    );
    assert!(
        headers["cache-control"]
            .to_str()
            .unwrap()
            .contains("no-store")
    );
    let observation = &before["metadata"]["selected_observation"];
    assert!(
        result
            == json!({
                "recording_id": library.recording,
                "items": [{
                    "observation_id": library.observation,
                    "installation_id": observation["plugin_installation_id"],
                    "installation_revision": library.installation_revision,
                    "provider_recording": provider::reference(),
                    "label": "Owned lyric provider",
                }],
            }),
        "The local read must expose only exact current approved provider identity and revision"
    );
    provider.assert_lookups(0).await;
    assert!(
        library.snapshot().await == before,
        "Listing lyric providers must preserve all public library and private collection state"
    );
    library.assert_original();
    capture.assert_core_outcomes(&[]);
    capture.assert_provider_lists(&["read"]);
    assert_private_logs(&capture, &library, provider, &[], &[]);
}

#[tokio::test]
async fn a_current_attached_lyric_identity_is_selectable_without_reusing_its_historical_observation()
 {
    let provider = provider::Provider::start().await;
    let attached = attached::Provider::start().await;
    let result = tokio::time::timeout(
        Duration::from_secs(30),
        AssertUnwindSafe(list_attached(&provider, &attached)).catch_unwind(),
    )
    .await;
    attached.finish().await;
    provider.finish().await;
    match result {
        Ok(Ok(())) => {}
        Ok(Err(panic)) => resume_unwind(panic),
        Err(_) => {
            panic!("The owned current attached lyric workflow must finish within its deadline")
        }
    }
}

async fn list_attached(provider: &provider::Provider, attached: &attached::Provider) {
    let library = Library::prepare(provider).await;
    let primary = library
        .account
        .get(
            &library.app,
            &format!("/api/v1/recordings/{}/metadata", library.recording),
        )
        .await;
    let installation = library
        .account
        .connect(
            &library.app,
            &attached.url,
            attached::NAME,
            Some(attached::TOKEN),
        )
        .await;
    let candidate = json!({"installation_id": installation["id"],
        "installation_revision": installation["revision"], "provider_recording_id": attached::RECORDING});
    let (status, review) = library
        .account
        .request(
            &library.app,
            "POST",
            &format!("/api/v1/recordings/{}/sources/review", library.recording),
            candidate.clone(),
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    let (status, _) = library
        .account
        .request(
            &library.app,
            "POST",
            &format!("/api/v1/recordings/{}/sources/attach", library.recording),
            json!({"candidate": candidate, "review_stamp": review["review_stamp"]}),
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    attached.assert_calls(2, 2, 0);
    let old = library
        .account
        .get(
            &library.app,
            &format!("/api/v1/recordings/{}/metadata", library.recording),
        )
        .await;
    let old_observation = old["observations"]["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|item| item["provider_recording_id"] == attached::RECORDING)
        .unwrap()["id"]
        .clone();
    attached.advance();
    let (status, adopted) = library.account.request(&library.app, "POST",
        &format!("/api/v1/plugins/{}/recordings", installation["id"].as_str().unwrap()),
        json!({"installation_revision": installation["revision"], "provider_recording_id": attached::RECORDING})).await;
    assert_eq!(status, StatusCode::OK);
    assert!(adopted["id"] == library.recording);
    attached.assert_calls(3, 3, 0);
    let before = library.snapshot().await;
    assert!(before["metadata"]["selected_observation"] == primary["selected_observation"]);
    assert!(
        before["metadata"]["title"] == primary["title"]
            && before["metadata"]["artist"] == primary["artist"]
    );
    let history = before["metadata"]["observations"]["items"]
        .as_array()
        .unwrap();
    let current = history
        .iter()
        .find(|item| {
            item["provider_recording_id"] == attached::RECORDING
                && item["provider_revision"] == json!(attached.reference().revision)
        })
        .unwrap();
    assert!(
        current["id"] != old_observation
            && history.iter().any(|item| item["id"] == old_observation)
    );
    let capture = logs::Logs::default();
    let subscriber = tracing::Dispatch::new(capture.subscriber());
    let (status, _, list) = fixture::exchange(
        &library.app,
        "GET",
        &format!("{}/providers", library.lookup_path()),
        Value::Null,
        library.account.headers(),
    )
    .with_subscriber(subscriber.clone())
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(
        list == json!({"recording_id": library.recording, "items": [
            {"observation_id": library.observation, "installation_id": primary["selected_observation"]["plugin_installation_id"],
             "installation_revision": library.installation_revision, "provider_recording": provider::reference(), "label": "Owned lyric provider"},
            {"observation_id": current["id"], "installation_id": installation["id"], "installation_revision": installation["revision"],
             "provider_recording": attached.reference(), "label": attached::NAME}
        ]}),
        "Only the current attached identity may be offered alongside the selected metadata identity"
    );
    attached.assert_calls(3, 3, 0);
    provider.assert_lookups(0).await;
    for (observation, expected) in [
        (old_observation.clone(), StatusCode::NOT_FOUND),
        (current["id"].clone(), StatusCode::OK),
    ] {
        let (status, _, result) = fixture::exchange(&library.app, "POST", &library.lookup_path(),
            json!({"observation_id": observation, "installation_revision": installation["revision"]}), library.account.headers())
            .with_subscriber(subscriber.clone()).await;
        assert_eq!(status, expected);
        if status == StatusCode::OK {
            let expected_document = LyricsResponse {
                recording: attached.reference(),
                lyrics: None,
            };
            let expected_digest = format!(
                "{:x}",
                Sha256::digest(serde_json::to_vec(&expected_document).unwrap())
            );
            assert!(
                result
                    == json!({"status": "ready", "recording_id": library.recording,
                "observation_id": current["id"], "document_digest": expected_digest,
                "document": expected_document})
            );
        } else {
            assert!(result.get("document").is_none());
            attached.assert_calls(3, 3, 0);
        }
    }
    attached.assert_calls(3, 3, 1);
    provider.assert_lookups(0).await;
    assert!(library.snapshot().await == before);
    library.assert_original();
    capture.assert_provider_lists(&["read"]);
    capture.assert_core_outcomes(&["not_found", "not_found"]);
    let reference = attached.reference();
    assert_private_logs(
        &capture,
        &library,
        provider,
        &["not_found", "not_found"],
        &[
            attached::RECORDING,
            attached::TOKEN,
            attached::TITLE,
            &attached.url,
            reference.revision.as_deref().unwrap(),
            current["id"].as_str().unwrap(),
            old_observation.as_str().unwrap(),
        ],
    );
}
