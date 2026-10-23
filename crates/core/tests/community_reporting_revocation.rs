#![cfg(unix)]

use hito_core::test_support::community_recovery as fixture;
use hito_core::test_support::community_service as community;

use hito_core::{
    CommunityCredentialReset, CommunityReportAdmission, Services, SuccessfulPlayback,
    UpdateCommunityPreferences,
};
use std::{future::pending, time::Duration};
use uuid::Uuid;

#[tokio::test]
async fn reset_during_held_provisioning_cannot_republish_credentials_or_touch_the_peer() {
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
        admit(&hito, peer, accepted).await,
        CommunityReportAdmission::Queued
    );
    assert!(worker(&hito).await);
    let retained_receipts = remote.accepted_receipts().await;
    assert_eq!(retained_receipts.len(), 1);
    let initial = remote.requests().await;
    assert_eq!(initial.len(), 2);
    assert_eq!(initial[1].path, "/v1/outcomes");
    let before = [
        owner.private_state(&hito).await,
        peer.private_state(&hito).await,
    ];

    let held = remote.hold_provision().await;
    let cancelled = Uuid::new_v4();
    assert_eq!(
        admit(&hito, owner, cancelled).await,
        CommunityReportAdmission::Queued
    );
    let delivery = hito.community.run_next_report_until(pending());
    tokio::pin!(delivery);
    tokio::select! {
        result = &mut delivery => panic!("Delivery must stay pending at the owned provisioning barrier: {result:?}"),
        _ = held.entered() => {},
    }
    assert_eq!(
        hito.community
            .reset_credentials(&owner.actor)
            .await
            .unwrap(),
        CommunityCredentialReset {
            reporters_removed: 0,
            reports_cancelled: 1,
        }
    );
    held.release();
    assert!(
        tokio::time::timeout(Duration::from_secs(3), &mut delivery)
            .await
            .unwrap()
            .unwrap()
    );
    assert_eq!(
        hito.community
            .reset_credentials(&owner.actor)
            .await
            .unwrap(),
        CommunityCredentialReset {
            reporters_removed: 0,
            reports_cancelled: 0,
        }
    );
    assert_eq!(
        admit(&hito, owner, cancelled).await,
        CommunityReportAdmission::Ignored
    );
    assert!(!worker(&hito).await);
    assert_eq!(remote.requests().await.len(), 3);
    assert_eq!(remote.requests().await[2].path, "/v1/admin/reporters");
    assert_eq!(remote.accepted_receipts().await, retained_receipts);
    assert_eq!(
        admit(&hito, peer, accepted).await,
        CommunityReportAdmission::AlreadyAccepted
    );
    assert_eq!(remote.requests().await.len(), 3);

    assert_eq!(
        admit(&hito, peer, Uuid::new_v4()).await,
        CommunityReportAdmission::Queued
    );
    assert!(worker(&hito).await);
    let requests = remote.requests().await;
    assert_eq!(requests.len(), 4);
    assert_eq!(requests[3].path, "/v1/outcomes");
    assert!(
        requests[3].authorization == initial[1].authorization,
        "Resetting another account must preserve the peer's retained reporter credential"
    );
    let final_receipts = remote.accepted_receipts().await;
    assert_eq!(final_receipts.len(), 2);
    assert!(final_receipts.contains(&retained_receipts[0]));
    assert_eq!(owner.private_state(&hito).await, before[0]);
    assert_eq!(peer.private_state(&hito).await, before[1]);
    library.assert_originals();
}

async fn admit(
    hito: &Services,
    account: &fixture::Account,
    submission_id: Uuid,
) -> CommunityReportAdmission {
    hito.community
        .observe_success(
            &account.actor,
            &account.source.id,
            SuccessfulPlayback { submission_id },
        )
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
