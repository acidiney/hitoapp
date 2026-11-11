use super::setup::Prepared;
use crate::{lrclib, provider};
use axum::http::StatusCode;
use hito_core::AutomaticLyricsLookup;
use hito_testkit::logs::LogCapture;
use serde_json::json;
use sha2::{Digest, Sha256};
use tracing::instrument::WithSubscriber;

#[tokio::test]
async fn automatic_workflow_logs_correlated_static_outcomes_without_metadata_credentials_or_documents()
 {
    let prepared = Prepared::new().await;
    let token = "owned-automatic-private-credential-canary";
    prepared.provider.require_bearer(token);
    let (status, refreshed) = prepared.library.account.request(&prepared.library.app, "POST", &format!("/api/v1/plugins/{}/refresh", prepared.installation["id"].as_str().unwrap()), json!({
        "revision":prepared.installation["revision"],"accept_manifest_digest":null,"credential":{"action":"replace","bearer_token":token},
    })).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(refreshed["kind"], "updated");
    assert_eq!(refreshed["installation"]["credential_set"], true);
    let actor = prepared.actor().await;
    let capture = LogCapture::with_limit(262144);
    let dispatch = capture.dispatch_with_spans("off,hito_core=trace,hito_server=trace");
    let (digest, wire) = async {
        let ready = prepared.read().await;
        assert_eq!(ready["status"], "ready_v2");
        prepared
            .provider
            .status("search", StatusCode::SERVICE_UNAVAILABLE);
        assert_eq!(prepared.read().await["status"], "unavailable");
        prepared.provider.status("search", StatusCode::OK);
        let mut other = provider::recording();
        other["id"] = json!("owned:other-logging-identity");
        prepared.provider.reply(
            "search",
            json!({"items":[provider::recording(),other],"next_cursor":null}),
        );
        assert_eq!(prepared.read().await["status"], "ambiguous");
        assert!(matches!(
            prepared
                .library
                .services
                .catalog
                .automatic_recording_lyrics_until(&actor, &prepared.library.recording, async {})
                .await,
            Ok(AutomaticLyricsLookup::Cancelled)
        ));
        (
            ready["document_digest"].as_str().unwrap().to_owned(),
            serde_json::to_string(&ready["document"]).unwrap(),
        )
    }
    .with_subscriber(dispatch)
    .await;
    let headers = prepared.library.account.headers();
    let cookie = headers["cookie"].to_str().unwrap();
    let session = cookie.strip_prefix("hito_session=").unwrap();
    let session_hash = format!("{:x}", Sha256::digest(session.as_bytes()));
    let query = format!("{} {}", lrclib::ARTIST, lrclib::TITLE);
    capture.assert_excludes(&[
        lrclib::TITLE,
        lrclib::ARTIST,
        &query,
        token,
        cookie,
        session,
        &session_hash,
        prepared.provider.url(),
        &prepared.library.recording,
        "owned:automatic",
        "owned:auto-doc",
        "Owned automatic protocol text",
        &digest,
        &wire,
    ]);
    let events: Vec<_> = capture
        .events()
        .into_iter()
        .filter(|event| event["fields"]["message"] == "Automatic recording lyric lookup completed")
        .collect();
    assert_eq!(events.len(), 4);
    for outcome in ["ready", "unavailable", "ambiguous", "cancelled"] {
        let matching: Vec<_> = events
            .iter()
            .filter(|event| event["fields"]["outcome"] == outcome)
            .collect();
        assert_eq!(matching.len(), 1);
        let event = matching[0];
        assert_eq!(event["level"], "DEBUG");
        assert_eq!(event["span"]["operation"], "automatic_recording_lyrics");
        assert!(event["fields"]["elapsed_ms"].as_u64().is_some());
        assert!(
            event["span"]["request_id"]
                .as_str()
                .is_some_and(|id| uuid::Uuid::parse_str(id).is_ok_and(|id| !id.is_nil()))
        );
    }
    prepared.finish().await;
}
