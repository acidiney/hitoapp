#![cfg(feature = "test-support")]

#[path = "support/restart_catalog_entries.rs"]
pub mod entries;
#[path = "../../hito-debrid-torbox/tests/audio/fixtures.rs"]
pub mod fixtures;
#[path = "../../hito-plugin-all-debrid/tests/fixtures/http.rs"]
pub mod http;
#[path = "../../hito-debrid-torbox/tests/audio/owned_tls.rs"]
pub mod owned_tls;
#[path = "../../hito-debrid-torbox/tests/support/mod.rs"]
pub mod provider;
#[path = "support/restart_workflow.rs"]
pub mod ready_workflow;
#[path = "../../hito-debrid-torbox/tests/audio/scripted_media.rs"]
pub mod scripted_media;
#[path = "support/state_inputs.rs"]
mod state_inputs;
#[path = "support/restart_catalog_workflow.rs"]
pub mod workflow;

use hito_plugin_torbox::{InitError, StateError};
use std::{fs::OpenOptions, io::Write, time::Duration};

pub const COOKIE: &str = "owned-private-torbox-state-key-cookie";

#[tokio::test]
async fn different_valid_key_is_rejected_without_harming_original_ready_receipt() {
    tokio::time::timeout(Duration::from_secs(10), async {
        let alternate = tempfile::tempdir().unwrap();
        let key_path = alternate.path().join("alternate.key");
        let mut options = OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut key = options.open(&key_path).unwrap();
        key.write_all(&[74_u8; 32]).unwrap();
        key.sync_all().unwrap();
        drop(key);

        let fixture = workflow::Fixture::start_removal().await;
        let first = fixture.open(false).await.expect("the original private key must open its state");
        let saved = first.prepare().await;
        workflow::assert_ready_status(first.status(&saved).await, &saved);
        workflow::assert_ready_replay(first.replay(&saved).await, &saved);
        let first_get_started = tokio::time::Instant::now();
        ready_workflow::complete(first.audio().await).await;
        let first_get_finished = tokio::time::Instant::now();
        first.close().await;

        let media_before_rejection = fixture.media_requests();
        let rejection_started = tokio::time::Instant::now();
        let rejected_outcome = match fixture.open_using_key(key_path).await {
            Err(InitError::State(StateError::InvalidState)) => "invalid_state",
            Err(_) => "other_error",
            Ok(unexpected) => {
                unexpected.close().await;
                "opened"
            }
        };
        let rejection_finished = tokio::time::Instant::now();
        let media_after_rejection = fixture.media_requests();
        eprintln!("state_key_rejection outcome={rejected_outcome} media_before={media_before_rejection} media_after={media_after_rejection}");

        let replacement = fixture.open(false).await.expect("the retained original key must still reopen its unchanged state");
        workflow::assert_ready_status(replacement.status(&saved).await, &saved);
        workflow::assert_ready_replay(replacement.replay(&saved).await, &saved);
        let replacement_get_started = tokio::time::Instant::now();
        ready_workflow::complete(replacement.audio().await).await;
        let replacement_get_finished = tokio::time::Instant::now();
        workflow::assert_ready_status(replacement.status(&saved).await, &saved);
        workflow::assert_ready_replay(replacement.replay(&saved).await, &saved);
        replacement.close().await;
        let (requests, media_requests) = fixture.close().await;

        let before_requests = requests.iter().filter(|request| request.received_at < rejection_started).count();
        let after_requests = requests.iter().filter(|request| request.received_at <= rejection_finished).count();
        let before_mints = requests.iter().filter(|request| request.received_at < rejection_started
            && request.target.starts_with("/v1/api/torrents/requestdl?")).count();
        let after_mints = requests.iter().filter(|request| request.received_at <= rejection_finished
            && request.target.starts_with("/v1/api/torrents/requestdl?")).count();
        eprintln!("state_key_evidence outcome={rejected_outcome} provider_before={before_requests} provider_after={after_requests} mint_before={before_mints} mint_after={after_mints} media_requests={media_requests}");
        assert_eq!(rejected_outcome, "invalid_state", "a different valid private key must not reopen existing state");
        assert_eq!((before_requests, before_mints), (3, 1));
        assert_eq!((after_requests, after_mints), (3, 1));
        assert_eq!((media_before_rejection, media_after_rejection), (1, 1));
        ready_workflow::assert_requests(&requests, [
            (first_get_started, first_get_finished),
            (replacement_get_started, replacement_get_finished),
        ]);
        assert_eq!(media_requests, 2);
    }).await.expect("the public state-key integrity workflow exceeded its deadline");
}
