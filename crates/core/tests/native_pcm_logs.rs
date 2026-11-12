#[path = "native_pcm/fixture.rs"]
#[allow(dead_code)]
mod fixture;

use hito_core::test_support::metadata_tag_logs::CapturedLogs;
use std::time::Duration;
use tracing::instrument::WithSubscriber;

#[tokio::test]
async fn native_decode_logs_safe_lifecycle_outcomes_without_private_input_or_diagnostics() {
    let logs = CapturedLogs::default();
    async {
        let library = fixture::library(3, fixture::no_configuration).await;
        let mut stream = library
            .grant()
            .await
            .open_pcm(Duration::from_secs(1))
            .await
            .unwrap();
        while stream.next_block().await.unwrap().is_some() {}
        stream.close().await.unwrap();
        let failed =
            fixture::scripted("printf 'private-decoder-stderr-and-signed-url' >&2; exit 7").await;
        assert!(failed.grant().await.open_pcm(Duration::ZERO).await.is_err());
        let text = logs.text();
        for evidence in [
            "native_pcm_started",
            "native_pcm_closed",
            "playback_grant",
            "account_id",
            "complete",
            "failed",
        ] {
            assert!(text.contains(evidence), "missing {evidence}");
        }
        for private in [
            "private-recording",
            "private-decoder-stderr",
            "signed-url",
            library.directory.path().to_str().unwrap(),
            failed.directory.path().to_str().unwrap(),
            "/dev/stdin",
            "pipe:1",
        ] {
            assert!(
                !text.contains(private),
                "private decoder data leaked: {private}"
            );
        }
    }
    .with_subscriber(logs.subscriber())
    .await;
}
