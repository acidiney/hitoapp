#![cfg(feature = "test-support")]

pub use hito_debrid_torbox::test_support::fixtures;
pub use hito_debrid_torbox::test_support::owned_tls;
pub use hito_debrid_torbox::test_support::provider;
pub use hito_debrid_torbox::test_support::scripted_media;
#[path = "support/persistence_workflow.rs"]
pub mod workflow;

use hito_debrid_torbox::{CandidateContext, CandidateKey, MediaAllowlist};
use std::time::Duration;
use uuid::Uuid;
use workflow::{Fixture, assert_requests, complete};

const KEY: [u8; 32] = [0x53; 32];

#[tokio::test]
async fn sealed_candidate_restores_with_new_owners_and_revalidates_two_exact_wav_opens() {
    tokio::time::timeout(Duration::from_secs(10), async {
        let fixture = Fixture::start().await;
        let binding = fixtures::binding();
        let state_id = Uuid::new_v4();
        let operation_id = Uuid::new_v4();
        let handle = Uuid::new_v4();
        let context = || CandidateContext {
            state_id,
            operation_id,
            handle,
            recording_id: "owned-candidate-persistence-recording",
            source_id: "owned-candidate-persistence-source",
            binding: &binding,
        };
        let first = fixture.client();
        let candidate = first
            .prepare_bound_file(&binding)
            .await
            .expect("the owned completed file must mint one private candidate");
        let key = CandidateKey::new(KEY).unwrap();
        let sealed = first.seal_candidate(&key, context(), &candidate);
        drop(candidate);
        drop(first);
        drop(key);
        let sealed = match sealed {
            Ok(sealed) => sealed,
            Err(error) => {
                let _ = fixture.close().await;
                panic!("the minted private candidate must seal for persistence: {error:?}");
            }
        };

        let second = fixture.client();
        let key = CandidateKey::new(KEY).unwrap();
        let restored = second
            .restore_candidate(&key, context(), sealed.as_bytes())
            .expect("the sealed candidate must restore with a new client and key owner");
        assert!(restored.binding() == &binding);
        let allowlist =
            MediaAllowlist::new(vec![format!("https://{}", fixtures::MEDIA_HOST)]).unwrap();
        let first_open = complete(&second, &restored, &allowlist).await;
        let second_open = complete(&second, &restored, &allowlist).await;
        drop(restored);
        drop(second);
        drop(key);
        let (requests, media_requests) = fixture.close().await;
        assert_requests(&requests, [first_open, second_open]);
        assert_eq!(media_requests, 2);
    })
    .await
    .expect("the owned TorBox candidate-persistence workflow exceeded its deadline");
}
