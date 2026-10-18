#![cfg(unix)]

#[path = "fixtures/community_service.rs"]
mod community;
#[path = "fixtures/community_recovery.rs"]
mod fixture;
#[path = "fixtures/content_service.rs"]
mod provider;
mod support;

use hito_core::{
    Actor, CommunityReportAdmission, Services, SuccessfulPlayback, UpdateCommunityPreferences,
};
use serde_json::Value;
use std::{future::pending, time::Duration};
use uuid::Uuid;

#[derive(Clone, Copy)]
enum Revocation {
    Consent,
    Session,
}

#[tokio::test]
async fn disabling_playback_consent_cancels_a_held_receipt_without_retrying_or_clearing_the_peer() {
    held_receipt(Revocation::Consent).await;
}

#[tokio::test]
async fn ending_the_listening_session_cancels_a_held_receipt_without_retrying_or_clearing_the_peer()
{
    held_receipt(Revocation::Session).await;
}

async fn held_receipt(revoke: Revocation) {
    let remote = community::CommunityServer::start().await;
    let (library, hito) = fixture::Library::prepare(&remote).await;
    for (index, account) in library.accounts.iter().enumerate() {
        hito.community
            .update_preferences(
                &account.actor,
                UpdateCommunityPreferences {
                    revision: 0,
                    playback_reporting: true,
                    metadata_corrections: index == 0,
                },
            )
            .await
            .unwrap();
    }
    let owner = &library.accounts[0];
    let peer = &library.accounts[1];
    let accepted = Uuid::new_v4();
    assert_eq!(
        admit(&hito, &peer.actor, &peer.source.id, accepted).await,
        CommunityReportAdmission::Queued
    );
    assert!(worker(&hito).await);
    let initial = remote.requests().await;
    assert_eq!(initial.len(), 2);
    let before = [
        collection(owner.private_state(&hito).await),
        peer.private_state(&hito).await,
    ];
    let listening_session = match revoke {
        Revocation::Session => Some(
            hito.identity
                .login(fixture::credentials("recovery-admin"))
                .await
                .unwrap(),
        ),
        Revocation::Consent => None,
    };
    let listening = match &listening_session {
        Some(session) => hito.identity.authenticate(&session.token).await.unwrap(),
        None => owner.actor.clone(),
    };
    let held = remote.hold_outcome().await;
    let cancelled = Uuid::new_v4();
    assert_eq!(
        admit(&hito, &listening, &owner.source.id, cancelled).await,
        CommunityReportAdmission::Queued
    );
    let delivery = hito.community.run_next_report_until(pending());
    tokio::pin!(delivery);
    tokio::select! {
        result = &mut delivery => panic!("Delivery must remain at the owned receipt barrier: {result:?}"),
        _ = held.entered() => {},
    }
    let retained = remote.accepted_receipts().await;
    assert_eq!(retained.len(), 2);
    let request_count = remote.requests().await.len();
    assert_eq!(request_count, 4);
    match revoke {
        Revocation::Consent => {
            let current = hito.community.preferences(&owner.actor).await.unwrap();
            hito.community
                .update_preferences(
                    &owner.actor,
                    UpdateCommunityPreferences {
                        revision: current.revision,
                        playback_reporting: false,
                        metadata_corrections: current.metadata_corrections,
                    },
                )
                .await
                .unwrap();
        }
        Revocation::Session => hito
            .identity
            .logout(&listening_session.as_ref().unwrap().token)
            .await
            .unwrap(),
    }
    held.release();
    assert!(
        tokio::time::timeout(Duration::from_secs(3), &mut delivery)
            .await
            .unwrap()
            .unwrap()
    );
    assert!(!worker(&hito).await);
    assert_eq!(remote.requests().await.len(), request_count);
    assert_eq!(remote.accepted_receipts().await, retained);
    let current = hito.community.preferences(&owner.actor).await.unwrap();
    assert!(current.metadata_corrections);
    if matches!(revoke, Revocation::Consent) {
        assert!(!current.playback_reporting);
        hito.community
            .update_preferences(
                &owner.actor,
                UpdateCommunityPreferences {
                    revision: current.revision,
                    playback_reporting: true,
                    metadata_corrections: current.metadata_corrections,
                },
            )
            .await
            .unwrap();
    } else {
        assert!(current.playback_reporting);
        assert!(
            hito.identity
                .authenticate(&listening_session.as_ref().unwrap().token)
                .await
                .is_err()
        );
    }
    assert_eq!(
        admit(&hito, &owner.actor, &owner.source.id, cancelled).await,
        CommunityReportAdmission::Ignored
    );
    assert_eq!(
        admit(&hito, &peer.actor, &peer.source.id, accepted).await,
        CommunityReportAdmission::AlreadyAccepted
    );
    assert!(!worker(&hito).await);
    assert_eq!(remote.requests().await.len(), request_count);
    assert_eq!(
        admit(&hito, &peer.actor, &peer.source.id, Uuid::new_v4()).await,
        CommunityReportAdmission::Queued
    );
    assert!(worker(&hito).await);
    let requests = remote.requests().await;
    assert_eq!(requests.len(), request_count + 1);
    assert_eq!(requests.last().unwrap().path, "/v1/outcomes");
    assert!(
        requests.last().unwrap().authorization == initial[1].authorization,
        "Revoking another account must preserve the peer's retained reporter"
    );
    let receipts = remote.accepted_receipts().await;
    assert_eq!(receipts.len(), 3);
    assert!(retained.iter().all(|receipt| receipts.contains(receipt)));
    assert_eq!(collection(owner.private_state(&hito).await), before[0]);
    assert_eq!(peer.private_state(&hito).await, before[1]);
    library.assert_originals();
}

fn collection(mut state: Value) -> Value {
    state.as_object_mut().unwrap().remove("preferences");
    state
}

async fn admit(
    hito: &Services,
    actor: &Actor,
    source: &str,
    submission_id: Uuid,
) -> CommunityReportAdmission {
    hito.community
        .observe_success(actor, source, SuccessfulPlayback { submission_id })
        .await
        .unwrap()
}

async fn worker(hito: &Services) -> bool {
    tokio::time::timeout(
        Duration::from_secs(5),
        hito.community.run_next_report_until(pending()),
    )
    .await
    .unwrap()
    .unwrap()
}
