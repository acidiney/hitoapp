#![cfg(feature = "test-support")]

pub use hito_debrid_torbox::test_support::fixtures;
pub use hito_debrid_torbox::test_support::owned_tls;
#[path = "support/persistence_logs.rs"]
mod persistence_logs;
pub use hito_debrid_torbox::test_support::provider;
pub use hito_debrid_torbox::test_support::scripted_media;
#[path = "support/persistence_workflow.rs"]
pub mod workflow;

use hito_debrid_torbox::{
    CandidateContext, CandidateKey, PersistenceError, SealedCandidate, TorrentFileBinding,
};
use persistence_logs::Capture;
use std::time::Duration;
use uuid::Uuid;
use workflow::Fixture;

type BindingChange = (&'static str, fn(&mut TorrentFileBinding));

struct Outcomes {
    private_ids: [Uuid; 3],
    restored_exact: bool,
    changed_bindings_valid: bool,
    sealed_bytes: usize,
    context_rejections: Vec<(&'static str, Option<PersistenceError>)>,
    input_rejections: Vec<(&'static str, Option<PersistenceError>)>,
}

#[tokio::test]
async fn every_persisted_context_field_and_invalid_envelope_rejects_locally_with_private_logs() {
    tokio::time::timeout(Duration::from_secs(10), async {
        let fixture = Fixture::start().await;
        let capture = Capture::default();
        let outcomes = exercise(&fixture, &capture).await;
        let (requests, media_requests) = fixture.close().await;
        let mint_requests = requests
            .iter()
            .filter(|request| request.target.starts_with("/v1/api/torrents/requestdl?"))
            .count();

        assert_eq!(requests.len(), 2);
        assert_eq!(mint_requests, 1);
        assert_eq!(media_requests, 0);
        let outcomes = outcomes.expect("the exact owned context must seal and restore locally");
        assert!(outcomes.restored_exact);
        assert!(outcomes.changed_bindings_valid);
        assert!((1..=SealedCandidate::MAX_BYTES).contains(&outcomes.sealed_bytes));
        assert_eq!(outcomes.context_rejections.len(), 11);
        assert_eq!(outcomes.input_rejections.len(), 5);
        for (field, error) in outcomes
            .context_rejections
            .iter()
            .chain(&outcomes.input_rejections)
        {
            assert!(
                *error == Some(PersistenceError::Rejected),
                "the {field} alteration must fail with the closed rejection"
            );
            assert!(
                error.unwrap().to_string() == "The private candidate state is invalid",
                "persistence errors must retain their closed message"
            );
        }
        let mut expected = vec![
            ("candidate_authority", "complete", 1),
            ("candidate_sealing", "sealed", 1),
            ("candidate_restoration", "restored", 1),
        ];
        expected.extend(std::iter::repeat_n(("candidate_restoration", "rejected", 0), 16));
        let event_count = capture.assert_events(&expected, &outcomes.private_ids);
        println!(
            "Owned persistence context: context_rejections={} input_rejections={} persistence_events={event_count} provider_requests={} mint_requests={mint_requests} media_requests={media_requests}",
            outcomes.context_rejections.len(), outcomes.input_rejections.len(), requests.len()
        );
    })
    .await
    .expect("the owned TorBox persistence-context workflow exceeded its deadline");
}

async fn exercise(fixture: &Fixture, capture: &Capture) -> Result<Outcomes, &'static str> {
    let binding = fixtures::binding();
    let private_ids = [Uuid::new_v4(), Uuid::new_v4(), Uuid::new_v4()];
    let context = || CandidateContext {
        state_id: private_ids[0],
        operation_id: private_ids[1],
        handle: private_ids[2],
        recording_id: "owned-private-context-recording",
        source_id: "owned-private-context-source",
        binding: &binding,
    };
    let key = CandidateKey::new([0x53; 32]).map_err(|_| "the owned key must configure")?;
    let client = fixture.client();
    let candidate = client
        .prepare_bound_file(&binding)
        .await
        .map_err(|_| "the completed owned file must mint one candidate")?;
    let dispatch = capture.dispatch();
    tracing::dispatcher::with_default(&dispatch, || {
        let _authority = client
            .candidate_authority(&key)
            .map_err(|_| "the private credential authority must compute locally")?;
        let sealed = client
            .seal_candidate(&key, context(), &candidate)
            .map_err(|_| "the exact owned context must seal")?;
        let restored_exact = client
            .restore_candidate(&key, context(), sealed.as_bytes())
            .is_ok_and(|restored| restored.binding() == &binding);
        let changed_contexts = [
            (
                "state_id",
                CandidateContext {
                    state_id: Uuid::new_v4(),
                    ..context()
                },
            ),
            (
                "operation_id",
                CandidateContext {
                    operation_id: Uuid::new_v4(),
                    ..context()
                },
            ),
            (
                "handle",
                CandidateContext {
                    handle: Uuid::new_v4(),
                    ..context()
                },
            ),
            (
                "recording_id",
                CandidateContext {
                    recording_id: "owned-other-recording",
                    ..context()
                },
            ),
            (
                "source_id",
                CandidateContext {
                    source_id: "owned-other-source",
                    ..context()
                },
            ),
        ];
        let mut context_rejections = Vec::with_capacity(11);
        for (field, changed) in changed_contexts {
            context_rejections.push((
                field,
                client
                    .restore_candidate(&key, changed, sealed.as_bytes())
                    .err(),
            ));
        }
        let binding_changes: [BindingChange; 6] = [
            ("torrent_id", |binding| binding.torrent_id += 1),
            ("info_hash", |binding| {
                binding.info_hash.replace_range(39..40, "8")
            }),
            ("torrent_bytes", |binding| binding.torrent_bytes += 2),
            ("file_id", |binding| binding.file_id += 1),
            ("file_name", |binding| {
                binding.file_name.push_str(".changed")
            }),
            ("file_bytes", |binding| binding.file_bytes -= 2),
        ];
        let mut changed_bindings_valid = true;
        for (field, change) in binding_changes {
            let mut changed = binding.clone();
            change(&mut changed);
            changed_bindings_valid &= changed.validate().is_ok();
            context_rejections.push((
                field,
                client
                    .restore_candidate(
                        &key,
                        CandidateContext {
                            binding: &changed,
                            ..context()
                        },
                        sealed.as_bytes(),
                    )
                    .err(),
            ));
        }
        let bytes = sealed.as_bytes();
        let mut unsupported = bytes.to_vec();
        *unsupported
            .first_mut()
            .ok_or("a sealed envelope must have a version")? ^= 0x80;
        let invalid_inputs = [
            ("empty", Vec::new()),
            ("truncated_envelope", bytes[..1].to_vec()),
            ("truncated_tag", bytes[..bytes.len() - 1].to_vec()),
            ("unsupported_version", unsupported),
            ("oversized", vec![1_u8; SealedCandidate::MAX_BYTES + 1]),
        ];
        let mut input_rejections = Vec::with_capacity(5);
        for (field, input) in invalid_inputs {
            input_rejections.push((
                field,
                client.restore_candidate(&key, context(), &input).err(),
            ));
        }
        Ok(Outcomes {
            private_ids,
            restored_exact,
            changed_bindings_valid,
            sealed_bytes: bytes.len(),
            context_rejections,
            input_rejections,
        })
    })
}
