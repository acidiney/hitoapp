#![cfg(unix)]

use hito_core::test_support::community_recovery as fixture;
use hito_core::test_support::community_service as community;

use hito_core::{
    CommunityCredentialReset, CommunityReportAdmission, Error, Services, SuccessfulPlayback,
    UpdateCommunityPreferences,
};
use std::{future::pending, time::Duration};
use uuid::Uuid;

#[tokio::test]
async fn each_account_explicitly_resets_its_community_credentials_before_lost_key_recovery() {
    let first_origin = community::CommunityServer::start().await;
    let second_origin = community::CommunityServer::start().await;
    let (library, hito) = fixture::Library::prepare(&first_origin).await;
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
        assert_eq!(
            hito.community
                .observe_success(
                    &account.actor,
                    &account.source.id,
                    SuccessfulPlayback {
                        submission_id: Uuid::new_v4()
                    }
                )
                .await
                .unwrap(),
            CommunityReportAdmission::Queued
        );
    }
    assert!(worker(&hito).await);
    assert!(worker(&hito).await);
    assert!(!worker(&hito).await);
    let first_receipts = first_origin.accepted_receipts().await;
    assert_eq!(first_receipts.len(), 2);
    let first_requests = first_origin.requests().await;
    let reporter_headers = first_requests
        .iter()
        .filter(|request| request.path == "/v1/outcomes")
        .map(|request| request.authorization.as_deref())
        .collect::<Vec<_>>();
    assert!(
        reporter_headers.len() == 2 && reporter_headers[0] != reporter_headers[1],
        "The owned service must issue distinct account reporters"
    );
    drop(hito);

    let hito = library.open(&second_origin).await;
    let retained_submission = Uuid::new_v4();
    let owner = &library.accounts[0];
    assert_eq!(
        hito.community
            .observe_success(
                &owner.actor,
                &owner.source.id,
                SuccessfulPlayback {
                    submission_id: retained_submission
                }
            )
            .await
            .unwrap(),
        CommunityReportAdmission::Queued
    );
    assert!(worker(&hito).await);
    let second_receipts = second_origin.accepted_receipts().await;
    assert_eq!(second_receipts.len(), 1);
    let pending_submissions = [Uuid::new_v4(), Uuid::new_v4()];
    for (account, submission_id) in library.accounts.iter().zip(pending_submissions) {
        assert_eq!(
            hito.community
                .observe_success(
                    &account.actor,
                    &account.source.id,
                    SuccessfulPlayback { submission_id }
                )
                .await
                .unwrap(),
            CommunityReportAdmission::Queued
        );
    }
    let revoked_session = hito
        .identity
        .login(fixture::credentials("recovery-admin"))
        .await
        .unwrap();
    let revoked_actor = hito
        .identity
        .authenticate(&revoked_session.token)
        .await
        .unwrap();
    hito.identity.logout(&revoked_session.token).await.unwrap();
    let before = [
        library.accounts[0].private_state(&hito).await,
        library.accounts[1].private_state(&hito).await,
    ];
    let request_counts = [
        first_origin.requests().await.len(),
        second_origin.requests().await.len(),
    ];
    let key_path = library.directory.path().join("hito.plugin-key");
    let lost_key = std::fs::read(&key_path).unwrap();
    drop(hito);
    std::fs::remove_file(&key_path).unwrap();

    let hito = library.open(&second_origin).await;
    assert_unusable(&hito, &library, &key_path).await;
    assert!(matches!(
        hito.community.reset_credentials(&revoked_actor).await,
        Err(Error::Unauthorized)
    ));
    for account in &library.accounts {
        let cleared = hito
            .plugins
            .clear_credential(
                &account.actor,
                &account.installation.id,
                account.installation.revision,
            )
            .await
            .unwrap();
        assert!(!cleared.credential_set);
    }
    drop(hito);
    let hito = library.open(&second_origin).await;
    assert_unusable(&hito, &library, &key_path).await;
    assert_eq!(
        hito.community
            .reset_credentials(&owner.actor)
            .await
            .unwrap(),
        CommunityCredentialReset {
            reporters_removed: 2,
            reports_cancelled: 1
        }
    );
    assert_eq!(
        hito.community
            .reset_credentials(&owner.actor)
            .await
            .unwrap(),
        CommunityCredentialReset {
            reporters_removed: 0,
            reports_cancelled: 0
        }
    );
    assert_eq!(library.accounts[0].private_state(&hito).await, before[0]);
    assert_eq!(library.accounts[1].private_state(&hito).await, before[1]);
    drop(hito);

    let hito = library.open(&second_origin).await;
    assert_unusable(&hito, &library, &key_path).await;
    assert_eq!(
        hito.community
            .reset_credentials(&library.accounts[1].actor)
            .await
            .unwrap(),
        CommunityCredentialReset {
            reporters_removed: 1,
            reports_cancelled: 1
        }
    );
    drop(hito);
    let hito = library.open(&second_origin).await;
    for account in &library.accounts {
        assert!(
            hito.plugins
                .settings(&account.actor)
                .await
                .unwrap()
                .credential_storage_ready
        );
    }
    assert!(
        std::fs::read(&key_path).unwrap() != lost_key,
        "Only deliberate clearing of all private ciphertext may permit a new key"
    );
    assert_eq!(
        hito.community
            .observe_success(
                &owner.actor,
                &owner.source.id,
                SuccessfulPlayback {
                    submission_id: retained_submission
                }
            )
            .await
            .unwrap(),
        CommunityReportAdmission::AlreadyAccepted
    );
    for (account, submission_id) in library.accounts.iter().zip(pending_submissions) {
        assert_eq!(
            hito.community
                .observe_success(
                    &account.actor,
                    &account.source.id,
                    SuccessfulPlayback { submission_id }
                )
                .await
                .unwrap(),
            CommunityReportAdmission::Ignored
        );
    }
    assert!(!worker(&hito).await);
    assert_eq!(library.accounts[0].private_state(&hito).await, before[0]);
    assert_eq!(library.accounts[1].private_state(&hito).await, before[1]);
    assert_eq!(first_origin.requests().await.len(), request_counts[0]);
    assert_eq!(second_origin.requests().await.len(), request_counts[1]);
    assert_eq!(first_origin.accepted_receipts().await, first_receipts);
    assert_eq!(second_origin.accepted_receipts().await, second_receipts);
    library.assert_originals();
}

async fn assert_unusable(hito: &Services, library: &fixture::Library, key: &std::path::Path) {
    assert!(hito.healthy().await);
    assert!(
        !key.exists(),
        "Unreadable reporter ciphertext must never be silently abandoned or replaced"
    );
    for account in &library.accounts {
        assert!(
            !hito
                .plugins
                .settings(&account.actor)
                .await
                .unwrap()
                .credential_storage_ready
        );
        assert!(
            hito.community
                .preferences(&account.actor)
                .await
                .unwrap()
                .playback_reporting
        );
    }
    assert!(!worker(hito).await);
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
