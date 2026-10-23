use hito_core::test_support::support;

use hito_core::{
    CommunityRankingPreferences, CoreConfig, Credentials, Error, HostDeviceId,
    UpdateCommunityPreferences, UpdateCommunityRankingPreferences,
};
use std::time::Duration;

#[tokio::test]
async fn ranking_preferences_have_independent_account_cas_and_survive_reopen() {
    tokio::time::timeout(Duration::from_secs(20), exercise())
        .await
        .expect("The owned account preference workflow must finish within its deadline");
}

async fn exercise() {
    let (directory, hito, admin, _, music) = support::library("ranking-preferences").await;
    hito.identity
        .create_user(&admin, credentials("listener"))
        .await
        .unwrap();
    let session = hito.identity.login(credentials("listener")).await.unwrap();
    let listener = hito.identity.authenticate(&session.token).await.unwrap();
    let disabled = CommunityRankingPreferences {
        revision: 0,
        use_source_rankings: false,
    };
    for actor in [&admin, &listener] {
        assert_eq!(
            hito.community.ranking_preferences(actor).await.unwrap(),
            disabled
        );
    }
    let admin_reports = hito
        .community
        .update_preferences(
            &admin,
            UpdateCommunityPreferences {
                revision: 0,
                playback_reporting: true,
                metadata_corrections: false,
            },
        )
        .await
        .unwrap();
    let listener_corrections = hito
        .community
        .update_preferences(
            &listener,
            UpdateCommunityPreferences {
                revision: 0,
                playback_reporting: false,
                metadata_corrections: true,
            },
        )
        .await
        .unwrap();
    for actor in [&admin, &listener] {
        assert_eq!(
            hito.community.ranking_preferences(actor).await.unwrap(),
            disabled
        );
    }
    let (enable, keep_disabled) = tokio::join!(
        hito.community.update_ranking_preferences(
            &admin,
            UpdateCommunityRankingPreferences {
                revision: 0,
                use_source_rankings: true,
            },
        ),
        hito.community.update_ranking_preferences(
            &admin,
            UpdateCommunityRankingPreferences {
                revision: 0,
                use_source_rankings: false,
            },
        ),
    );
    let winner = match (enable, keep_disabled) {
        (Ok(saved), Err(Error::Conflict(_))) => {
            assert!(saved.use_source_rankings);
            saved
        }
        (Err(Error::Conflict(_)), Ok(saved)) => {
            assert!(!saved.use_source_rankings);
            saved
        }
        _ => panic!("Exactly one concurrent account preference save must win its revision"),
    };
    assert_eq!(winner.revision, 1);
    assert_eq!(
        hito.community.ranking_preferences(&admin).await.unwrap(),
        winner
    );
    assert_eq!(
        hito.community.ranking_preferences(&listener).await.unwrap(),
        disabled
    );
    assert_eq!(
        hito.community.preferences(&admin).await.unwrap(),
        admin_reports
    );
    assert_eq!(
        hito.community.preferences(&listener).await.unwrap(),
        listener_corrections
    );
    let listener_rankings = hito
        .community
        .update_ranking_preferences(
            &listener,
            UpdateCommunityRankingPreferences {
                revision: 0,
                use_source_rankings: true,
            },
        )
        .await
        .unwrap();
    assert_eq!(
        listener_rankings,
        CommunityRankingPreferences {
            revision: 1,
            use_source_rankings: true,
        }
    );
    assert_eq!(
        hito.community.ranking_preferences(&admin).await.unwrap(),
        winner
    );
    let admin_rankings = hito
        .community
        .update_ranking_preferences(
            &admin,
            UpdateCommunityRankingPreferences {
                revision: winner.revision,
                use_source_rankings: true,
            },
        )
        .await
        .unwrap();
    assert_eq!(
        admin_rankings,
        CommunityRankingPreferences {
            revision: 2,
            use_source_rankings: true,
        }
    );
    assert!(matches!(
        hito.community
            .update_ranking_preferences(
                &admin,
                UpdateCommunityRankingPreferences {
                    revision: winner.revision,
                    use_source_rankings: false,
                },
            )
            .await,
        Err(Error::Conflict(_))
    ));
    let admin_no_reports = hito
        .community
        .update_preferences(
            &admin,
            UpdateCommunityPreferences {
                revision: admin_reports.revision,
                playback_reporting: false,
                metadata_corrections: false,
            },
        )
        .await
        .unwrap();
    let listener_reports = hito
        .community
        .update_preferences(
            &listener,
            UpdateCommunityPreferences {
                revision: listener_corrections.revision,
                playback_reporting: true,
                metadata_corrections: true,
            },
        )
        .await
        .unwrap();
    assert_eq!(
        hito.community.ranking_preferences(&admin).await.unwrap(),
        admin_rankings
    );
    assert_eq!(
        hito.community.ranking_preferences(&listener).await.unwrap(),
        listener_rankings
    );
    hito.identity.logout(&session.token).await.unwrap();
    assert!(matches!(
        hito.community.ranking_preferences(&listener).await,
        Err(Error::Unauthorized)
    ));
    assert!(matches!(
        hito.community
            .update_ranking_preferences(
                &listener,
                UpdateCommunityRankingPreferences {
                    revision: listener_rankings.revision,
                    use_source_rankings: false,
                },
            )
            .await,
        Err(Error::Unauthorized)
    ));
    assert_eq!(
        hito.community.ranking_preferences(&admin).await.unwrap(),
        admin_rankings
    );
    drop(hito);
    let reopened = hito_core::open(CoreConfig::new(
        directory.path().join("hito.db"),
        music,
        HostDeviceId::new("ranking-preferences").unwrap(),
    ))
    .await
    .unwrap();
    let admin_session = reopened.identity.login(credentials("admin")).await.unwrap();
    let admin = reopened
        .identity
        .authenticate(&admin_session.token)
        .await
        .unwrap();
    let session = reopened
        .identity
        .login(credentials("listener"))
        .await
        .unwrap();
    let listener = reopened
        .identity
        .authenticate(&session.token)
        .await
        .unwrap();
    assert_eq!(
        reopened
            .community
            .ranking_preferences(&admin)
            .await
            .unwrap(),
        admin_rankings
    );
    assert_eq!(
        reopened
            .community
            .ranking_preferences(&listener)
            .await
            .unwrap(),
        listener_rankings
    );
    assert_eq!(
        reopened.community.preferences(&admin).await.unwrap(),
        admin_no_reports
    );
    assert_eq!(
        reopened.community.preferences(&listener).await.unwrap(),
        listener_reports
    );
    let disabled = reopened
        .community
        .update_ranking_preferences(
            &admin,
            UpdateCommunityRankingPreferences {
                revision: admin_rankings.revision,
                use_source_rankings: false,
            },
        )
        .await
        .unwrap();
    assert_eq!(
        disabled,
        CommunityRankingPreferences {
            revision: 3,
            use_source_rankings: false,
        }
    );
    assert_eq!(
        reopened
            .community
            .ranking_preferences(&listener)
            .await
            .unwrap(),
        listener_rankings
    );
    assert_eq!(
        reopened.community.preferences(&admin).await.unwrap(),
        admin_no_reports
    );
    assert_eq!(
        reopened.community.preferences(&listener).await.unwrap(),
        listener_reports
    );
}

fn credentials(username: &str) -> Credentials {
    Credentials {
        username: username.into(),
        password: "indexing test password".into(),
    }
}
