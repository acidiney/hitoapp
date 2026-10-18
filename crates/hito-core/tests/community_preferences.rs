mod support;

use hito_core::{
    CommunityPreferences, CoreConfig, Credentials, Error, HostDeviceId, UpdateCommunityPreferences,
};

#[tokio::test]
async fn consent_is_explicit_independent_account_private_and_durable() {
    let (directory, hito, admin, _, music) = support::library("community-consent").await;
    let disabled = CommunityPreferences {
        revision: 0,
        playback_reporting: false,
        metadata_corrections: false,
    };
    assert_eq!(hito.community.preferences(&admin).await.unwrap(), disabled);

    hito.identity
        .create_user(&admin, credentials("listener"))
        .await
        .unwrap();
    let session = hito.identity.login(credentials("listener")).await.unwrap();
    let listener = hito.identity.authenticate(&session.token).await.unwrap();
    assert_eq!(
        hito.community.preferences(&listener).await.unwrap(),
        disabled
    );

    let reporting = hito
        .community
        .update_preferences(
            &admin,
            UpdateCommunityPreferences {
                revision: disabled.revision,
                playback_reporting: true,
                metadata_corrections: false,
            },
        )
        .await
        .unwrap();
    assert_eq!(
        reporting,
        CommunityPreferences {
            revision: 1,
            playback_reporting: true,
            metadata_corrections: false,
        }
    );
    assert_eq!(
        hito.community.preferences(&listener).await.unwrap(),
        disabled
    );

    let corrections = hito
        .community
        .update_preferences(
            &listener,
            UpdateCommunityPreferences {
                revision: disabled.revision,
                playback_reporting: false,
                metadata_corrections: true,
            },
        )
        .await
        .unwrap();
    assert_eq!(
        corrections,
        CommunityPreferences {
            revision: 1,
            playback_reporting: false,
            metadata_corrections: true,
        }
    );
    assert_eq!(hito.community.preferences(&admin).await.unwrap(), reporting);

    hito.identity.logout(&session.token).await.unwrap();
    assert!(matches!(
        hito.community.preferences(&listener).await,
        Err(Error::Unauthorized)
    ));
    assert!(matches!(
        hito.community
            .update_preferences(
                &listener,
                UpdateCommunityPreferences {
                    revision: corrections.revision,
                    playback_reporting: true,
                    metadata_corrections: false,
                },
            )
            .await,
        Err(Error::Unauthorized)
    ));

    drop(hito);
    let reopened = hito_core::open(CoreConfig::new(
        directory.path().join("hito.db"),
        music,
        HostDeviceId::new("community-consent").unwrap(),
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
        reopened.community.preferences(&admin).await.unwrap(),
        reporting
    );
    assert_eq!(
        reopened.community.preferences(&listener).await.unwrap(),
        corrections
    );
}

fn credentials(username: &str) -> Credentials {
    Credentials {
        username: username.into(),
        password: "indexing test password".into(),
    }
}
