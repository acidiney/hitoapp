use hito_core::test_support::content_service as fixture;
use hito_core::test_support::hang_guard::hang_guard;
use hito_core::test_support::support;

use futures_util::FutureExt;
use hito_core::{Action, Actor, Error, OfferPluginService, PreviewPluginInstallation, Services};
use serde_json::{Value, json};
use std::{
    future::pending,
    io::Write,
    panic::AssertUnwindSafe,
    sync::{Arc, Mutex},
    time::Duration,
};
use tracing::{Instrument, instrument::WithSubscriber};
use uuid::Uuid;

const UNKNOWN_SERVICE: &str =
    "/private-preview-log-path-sentinel/private-service?token=private-preview-id-token-sentinel";
const TOKEN: &str = "private-preview-bearer-token-sentinel";

#[derive(Clone, Default)]
struct CapturedLogs(Arc<Mutex<Vec<u8>>>);

impl Write for CapturedLogs {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        let mut captured = self.0.lock().unwrap();
        if bytes.len() > 65_536usize.saturating_sub(captured.len()) {
            return Err(std::io::Error::other(
                "preview log capture exceeded its limit",
            ));
        }
        captured.extend_from_slice(bytes);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

#[tokio::test]
async fn an_unknown_service_preview_logs_a_correlated_safe_outcome_without_private_input() {
    let (directory, hito, actor, _library) =
        hang_guard("the owned catalog must open", support::catalog()).await;
    let original_path = directory.path().join("music/Alpha.wav");
    let original = std::fs::read(&original_path).unwrap();
    let server = fixture::ProviderServer::start().await;
    let outcome = tokio::time::timeout(
        Duration::from_secs(10),
        AssertUnwindSafe(exercise(&hito, &actor, &server)).catch_unwind(),
    )
    .await;
    let settled =
        tokio::time::timeout(Duration::from_secs(5), hito.downloads.finish_downloads()).await;
    let stopped = tokio::time::timeout(Duration::from_secs(2), server.shutdown()).await;
    assert!(matches!(settled, Ok(Ok(()))), "managed work must settle");
    assert!(stopped.is_ok(), "the owned provider must stop and join");
    assert!(
        std::fs::read(original_path).unwrap() == original,
        "a rejected preview must preserve the owned original audio"
    );
    match outcome {
        Ok(Ok(())) => {}
        Ok(Err(panic)) => std::panic::resume_unwind(panic),
        Err(_) => panic!("the preview log exercise exceeded its deadline"),
    }
}

async fn exercise(hito: &Services, actor: &Actor, server: &fixture::ProviderServer) {
    let recording = hito
        .catalog
        .recordings(actor, Default::default())
        .await
        .unwrap()
        .items
        .remove(0);
    let recording_id = Uuid::parse_str(&recording.id).unwrap();
    support::synchronize(
        hito,
        actor,
        vec![
            Action::LikeSet {
                recording_id,
                liked: true,
            },
            Action::ProgressSet {
                recording_id,
                position_ms: 650,
                speed: 1.25,
            },
        ],
    )
    .await;
    hito.plugins
        .offer_service(
            actor,
            OfferPluginService {
                name: "Owned preview log fixture".into(),
                base_url: server.url.clone(),
            },
        )
        .await
        .unwrap();
    let before = private_state(hito, actor, &recording.id).await;
    let logs = CapturedLogs::default();
    let captured = logs.clone();
    hito_core::test_support::metadata_tag_logs::anchor_callsite_interest();
    let subscriber = tracing_subscriber::fmt()
        .with_env_filter("off,hito_core=trace")
        .with_ansi(false)
        .without_time()
        .json()
        .with_writer(move || captured.clone())
        .finish();
    let request_id = Uuid::new_v4();
    let result = async {
        hito.plugins
            .preview_installation(
                actor,
                PreviewPluginInstallation {
                    service_id: UNKNOWN_SERVICE.into(),
                    service_revision: 1,
                    bearer_token: Some(TOKEN.into()),
                },
            )
            .instrument(tracing::info_span!(
                target: "hito_core",
                "preview_log_request",
                request_id = %request_id
            ))
            .await
    }
    .with_subscriber(subscriber)
    .await;
    assert!(matches!(result, Err(Error::NotFound)));
    let output = String::from_utf8(logs.0.lock().unwrap().clone()).unwrap();
    let events = output
        .lines()
        .map(|line| serde_json::from_str::<Value>(line).expect("preview logs must be structured"))
        .collect::<Vec<_>>();
    let completed = events
        .iter()
        .find(|event| event["fields"]["message"] == "Plugin installation preview completed")
        .expect("a rejected preview must log its safe outcome");
    assert_eq!(completed["fields"]["outcome"], "not_found");
    assert!(completed["fields"]["elapsed_ms"].is_u64());
    assert!(
        completed["spans"]
            .as_array()
            .unwrap()
            .iter()
            .any(|span| span["request_id"] == request_id.to_string()),
        "preview outcomes must retain their request correlation"
    );
    for private in [UNKNOWN_SERVICE, TOKEN, "private-preview", &server.url] {
        assert!(
            !output.contains(private),
            "preview logs must omit unknown caller IDs, credentials and private paths"
        );
    }
    assert!(
        private_state(hito, actor, &recording.id).await == before,
        "a rejected preview must preserve installations and private musical state"
    );
    assert!(!hito.downloads.run_next_job_until(pending()).await.unwrap());
    for path in [
        "v1/manifest",
        "v1/search",
        "v1/recordings/detail",
        "v1/recordings/metadata",
        "v1/sources",
        "v1/playlists/detail",
        "v1/playlists/items",
        "v1/audio",
    ] {
        assert!(
            server.requests(path).await.is_empty(),
            "an unknown service must trigger no provider request"
        );
    }
}

async fn private_state(hito: &Services, actor: &Actor, recording: &str) -> Value {
    json!({
        "plugins": hito.plugins.settings(actor).await.unwrap(),
        "catalog": hito.catalog.recordings(actor, Default::default()).await.unwrap(),
        "sources": hito.catalog.sources(actor, recording).await.unwrap(),
        "activity": hito.collection.activity(actor).await.unwrap(),
        "events": hito.collection.events(actor, 0).await.unwrap()
    })
}
