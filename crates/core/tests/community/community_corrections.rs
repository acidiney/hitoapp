#![cfg(unix)]

use hito_core::test_support::community_recovery as fixture;
use hito_core::test_support::community_service as community;

use hito_core::{
    CommunityReportAdmission, DeleteAccount, Error, SameRecording, Services,
    UpdateCommunityPreferences,
};
use std::future::pending;
use uuid::Uuid;

async fn consent(hito: &Services, account: &fixture::Account, revision: i64, on: bool) {
    hito.community
        .update_preferences(
            &account.actor,
            UpdateCommunityPreferences {
                revision,
                playback_reporting: false,
                metadata_corrections: on,
            },
        )
        .await
        .unwrap();
}

async fn from_owner(
    hito: &Services,
    owner: &fixture::Account,
    source: &str,
    other: &str,
) -> Result<CommunityReportAdmission, Error> {
    hito.community
        .suggest_same_recording(&owner.actor, source, same(other))
        .await
}

fn same(other: &str) -> SameRecording {
    SameRecording {
        submission_id: Uuid::new_v4(),
        other_source_id: other.to_owned(),
    }
}

#[tokio::test]
async fn a_same_recording_suggestion_needs_consent_and_is_delivered_with_the_reporter_token() {
    let remote = community::CommunityServer::start().await;
    let (library, hito) = fixture::Library::prepare(&remote).await;
    let owner = &library.accounts[0];
    let youtube = owner.attach_youtube(&hito, &owner.actor).await;
    let suggest = |request: SameRecording| {
        hito.community
            .suggest_same_recording(&owner.actor, &owner.source.id, request)
    };
    assert_eq!(
        suggest(same(&youtube.source.id)).await.unwrap(),
        CommunityReportAdmission::Ignored
    );
    assert!(
        !hito
            .community
            .run_next_report_until(pending())
            .await
            .unwrap()
    );
    consent(&hito, owner, 0, true).await;
    let request = same(&youtube.source.id);
    let submission = request.submission_id;
    assert_eq!(
        suggest(request).await.unwrap(),
        CommunityReportAdmission::Queued
    );
    assert_eq!(
        suggest(SameRecording {
            submission_id: submission,
            other_source_id: youtube.source.id.clone(),
        })
        .await
        .unwrap(),
        CommunityReportAdmission::Queued
    );
    assert!(
        hito.community
            .run_next_report_until(pending())
            .await
            .unwrap()
    );
    let delivered = remote
        .requests()
        .await
        .into_iter()
        .filter(|request| request.path == "/v1/corrections")
        .collect::<Vec<_>>();
    assert_eq!(delivered.len(), 1);
    assert_eq!(
        delivered[0].authorization.as_deref(),
        Some(format!("Bearer {}", community::REPORTER_TOKEN).as_str())
    );
    assert_eq!(
        delivered[0].body,
        serde_json::json!({
            "submission_id": submission,
            "left": {"provider": "jamendo", "recording_id": "42"},
            "right": {"provider": "youtube", "recording_id": "PrivTone001"},
            "relation": "same_recording"
        })
    );
    assert_eq!(
        suggest(SameRecording {
            submission_id: submission,
            other_source_id: youtube.source.id.clone(),
        })
        .await
        .unwrap(),
        CommunityReportAdmission::AlreadyAccepted
    );
    assert!(matches!(
        suggest(SameRecording {
            submission_id: submission,
            other_source_id: owner.source.id.clone(),
        })
        .await,
        Err(Error::Invalid(_))
    ));
    assert!(
        !hito
            .community
            .run_next_report_until(pending())
            .await
            .unwrap()
    );
}

#[tokio::test]
async fn only_two_public_sources_of_one_recording_that_the_account_can_play_are_accepted() {
    let remote = community::CommunityServer::start().await;
    let (library, hito) = fixture::Library::prepare(&remote).await;
    let [owner, peer] = &library.accounts;
    let youtube = owner.attach_youtube(&hito, &owner.actor).await;
    consent(&hito, owner, 0, true).await;
    assert!(matches!(
        from_owner(&hito, owner, &owner.source.id, &peer.source.id).await,
        Err(Error::NotFound)
    ));
    assert!(matches!(
        from_owner(&hito, owner, &owner.source.id, "missing-source").await,
        Err(Error::NotFound)
    ));
    assert!(matches!(
        from_owner(&hito, owner, &owner.source.id, &owner.source.id).await,
        Err(Error::Invalid(_))
    ));
    let first = same(&youtube.source.id);
    let submission = first.submission_id;
    assert_eq!(
        hito.community
            .suggest_same_recording(&owner.actor, &owner.source.id, first)
            .await
            .unwrap(),
        CommunityReportAdmission::Queued
    );
    assert!(matches!(
        hito.community
            .suggest_same_recording(
                &owner.actor,
                &youtube.source.id,
                SameRecording {
                    submission_id: submission,
                    other_source_id: owner.source.id.clone(),
                },
            )
            .await,
        Err(Error::Conflict(_))
    ));
    consent(&hito, peer, 0, true).await;
    assert!(matches!(
        hito.community
            .suggest_same_recording(&peer.actor, &owner.source.id, same(&youtube.source.id))
            .await,
        Err(Error::NotFound)
    ));
}

#[tokio::test]
async fn pending_suggestions_are_capped_and_cancelled_by_turning_consent_off() {
    let remote = community::CommunityServer::start().await;
    let (library, hito) = fixture::Library::prepare(&remote).await;
    let owner = &library.accounts[0];
    let youtube = owner.attach_youtube(&hito, &owner.actor).await;
    consent(&hito, owner, 0, true).await;
    for _ in 0..16 {
        assert_eq!(
            hito.community
                .suggest_same_recording(&owner.actor, &owner.source.id, same(&youtube.source.id))
                .await
                .unwrap(),
            CommunityReportAdmission::Queued
        );
    }
    assert_eq!(
        hito.community
            .suggest_same_recording(&owner.actor, &owner.source.id, same(&youtube.source.id))
            .await
            .unwrap(),
        CommunityReportAdmission::Ignored
    );
    consent(&hito, owner, 1, false).await;
    assert!(
        !hito
            .community
            .run_next_report_until(pending())
            .await
            .unwrap()
    );
    assert!(
        remote
            .requests()
            .await
            .iter()
            .all(|request| request.path != "/v1/corrections")
    );
}

#[tokio::test]
async fn signing_out_cancels_and_deleting_the_account_removes_queued_suggestions() {
    let remote = community::CommunityServer::start().await;
    let (library, hito) = fixture::Library::prepare(&remote).await;
    let [admin, listener] = &library.accounts;
    let youtube = listener.attach_youtube(&hito, &admin.actor).await;
    consent(&hito, listener, 0, true).await;
    let session = hito
        .identity
        .login(fixture::credentials("recovery-listener"))
        .await
        .unwrap();
    let signed_in = hito.identity.authenticate(&session.token).await.unwrap();
    assert_eq!(
        hito.community
            .suggest_same_recording(&signed_in, &listener.source.id, same(&youtube.source.id))
            .await
            .unwrap(),
        CommunityReportAdmission::Queued
    );
    hito.identity.logout(&session.token).await.unwrap();
    assert!(
        !hito
            .community
            .run_next_report_until(pending())
            .await
            .unwrap()
    );
    hito.identity
        .delete_account(
            &admin.actor,
            &listener.actor.user().id,
            DeleteAccount {
                username: "recovery-listener".into(),
            },
        )
        .await
        .unwrap();
    assert!(
        remote
            .requests()
            .await
            .iter()
            .all(|request| request.path != "/v1/corrections")
    );
}
