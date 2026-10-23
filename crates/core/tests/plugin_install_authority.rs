use hito_core::test_support::content_service as fixture;
#[path = "fixtures/plugin_installation.rs"]
mod installation;
use hito_core::test_support::support;

use futures_util::FutureExt;
use hito_core::{CoreConfig, Error, HostDeviceId, OfferPluginService, PreviewPluginInstallation};
use installation::InstallationFixture;
use std::{panic::AssertUnwindSafe, time::Duration};

#[tokio::test]
async fn logout_during_a_held_preview_prevents_returning_a_declaration() {
    logout_case(false).await;
}

#[tokio::test]
async fn logout_during_a_held_accepted_installation_prevents_publication() {
    logout_case(true).await;
}

async fn logout_case(accept: bool) {
    let owned = tokio::time::timeout(Duration::from_secs(10), InstallationFixture::start())
        .await
        .expect("the owned fixture must start within its deadline");
    let outcome = tokio::time::timeout(
        Duration::from_secs(15),
        AssertUnwindSafe(async {
            let (actor, session) = owned.session().await;
            let digest = if accept {
                Some(
                    owned
                        .hito
                        .plugins
                        .preview_installation(&actor, owned.preview_request(None))
                        .await
                        .unwrap()
                        .digest,
                )
            } else {
                None
            };
            let gate = owned.server.hold("v1/manifest").await;
            let waiting = async {
                match digest.as_deref() {
                    Some(digest) => owned
                        .hito
                        .plugins
                        .install(&actor, owned.install_request(digest, None))
                        .await
                        .map(|_| ()),
                    None => owned
                        .hito
                        .plugins
                        .preview_installation(&actor, owned.preview_request(None))
                        .await
                        .map(|_| ()),
                }
            };
            let revoke = async {
                tokio::time::timeout(Duration::from_secs(3), gate.entered())
                    .await
                    .expect("the manifest must reach its owned barrier");
                tokio::time::timeout(Duration::from_secs(2), owned.hito.identity.logout(&session))
                    .await
                    .expect("logout must not wait for provider HTTP")
                    .unwrap();
                gate.open();
            };
            let (result, ()) = tokio::join!(waiting, revoke);
            assert!(matches!(result, Err(Error::Unauthorized)));
            assert!(
                owned
                    .hito
                    .plugins
                    .settings(&owned.actor)
                    .await
                    .unwrap()
                    .installations
                    .is_empty()
            );
            owned.assert_preserved().await;
            owned
                .assert_manifests_only(if accept { 2 } else { 1 })
                .await;
        })
        .catch_unwind(),
    )
    .await;
    owned.finish(outcome).await;
}

#[tokio::test]
async fn withdrawal_and_reoffer_during_a_held_preview_do_not_return_old_authority() {
    withdrawal_case(false).await;
}

#[tokio::test]
async fn withdrawal_and_reoffer_during_a_held_accepted_installation_do_not_publish_old_authority() {
    withdrawal_case(true).await;
}

async fn withdrawal_case(accept: bool) {
    let owned = tokio::time::timeout(Duration::from_secs(10), InstallationFixture::start())
        .await
        .expect("the owned fixture must start within its deadline");
    let outcome = tokio::time::timeout(
        Duration::from_secs(15),
        AssertUnwindSafe(async {
            let digest = if accept {
                Some(
                    owned
                        .hito
                        .plugins
                        .preview_installation(&owned.actor, owned.preview_request(None))
                        .await
                        .unwrap()
                        .digest,
                )
            } else {
                None
            };
            let gate = owned.server.hold("v1/manifest").await;
            let waiting = async {
                match digest.as_deref() {
                    Some(digest) => owned
                        .hito
                        .plugins
                        .install(&owned.actor, owned.install_request(digest, None))
                        .await
                        .map(|_| ()),
                    None => owned
                        .hito
                        .plugins
                        .preview_installation(&owned.actor, owned.preview_request(None))
                        .await
                        .map(|_| ()),
                }
            };
            let replace = async {
                tokio::time::timeout(Duration::from_secs(3), gate.entered())
                    .await
                    .expect("the manifest must reach its owned barrier");
                owned
                    .hito
                    .plugins
                    .withdraw_service(&owned.actor, &owned.service.id, owned.service.revision)
                    .await
                    .unwrap();
                let replacement = owned
                    .hito
                    .plugins
                    .offer_service(
                        &owned.actor,
                        OfferPluginService {
                            name: "Reoffered owned fixture".into(),
                            base_url: owned.server.url.clone(),
                        },
                    )
                    .await
                    .unwrap();
                gate.open();
                replacement
            };
            let (result, replacement) = tokio::join!(waiting, replace);
            if accept {
                assert!(matches!(result, Err(Error::Conflict(_))));
            } else {
                assert!(matches!(result, Err(Error::NotFound)));
            }
            assert_ne!(replacement.id, owned.service.id);
            let settings = owned.hito.plugins.settings(&owned.actor).await.unwrap();
            assert!(settings.installations.is_empty());
            assert_eq!(settings.services.len(), 1);
            assert_eq!(settings.services[0].id, replacement.id);
            owned
                .hito
                .plugins
                .preview_installation(
                    &owned.actor,
                    PreviewPluginInstallation {
                        service_id: replacement.id,
                        service_revision: replacement.revision,
                        bearer_token: None,
                    },
                )
                .await
                .unwrap();
            assert!(
                owned
                    .hito
                    .plugins
                    .settings(&owned.actor)
                    .await
                    .unwrap()
                    .installations
                    .is_empty()
            );
            owned.assert_preserved().await;
            owned
                .assert_manifests_only(if accept { 3 } else { 2 })
                .await;
        })
        .catch_unwind(),
    )
    .await;
    owned.finish(outcome).await;
}

#[tokio::test]
async fn an_accepted_installation_wins_while_an_older_preview_is_held() {
    let owned = tokio::time::timeout(Duration::from_secs(10), InstallationFixture::start())
        .await
        .expect("the owned fixture must start within its deadline");
    let outcome = tokio::time::timeout(
        Duration::from_secs(15),
        AssertUnwindSafe(async {
            let preview = owned
                .hito
                .plugins
                .preview_installation(&owned.actor, owned.preview_request(None))
                .await
                .unwrap();
            let concurrent = hito_core::open(CoreConfig::new(
                owned.directory.path().join("hito.db"),
                owned.directory.path().join("music"),
                HostDeviceId::new("server").unwrap(),
            ))
            .await
            .unwrap();
            let gate = owned.server.hold("v1/manifest").await;
            let waiting = owned
                .hito
                .plugins
                .preview_installation(&owned.actor, owned.preview_request(None));
            let publish = async {
                tokio::time::timeout(Duration::from_secs(3), gate.entered())
                    .await
                    .expect("the older preview must reach its owned barrier");
                owned.server.hold("v1/manifest").await.open();
                let winner = concurrent
                    .plugins
                    .install(
                        &owned.actor,
                        owned.install_request(
                            &preview.digest,
                            Some("private-concurrent-install-token"),
                        ),
                    )
                    .await
                    .unwrap();
                tokio::time::timeout(
                    Duration::from_secs(5),
                    concurrent.downloads.finish_downloads(),
                )
                .await
                .expect("the concurrent capability must settle")
                .unwrap();
                gate.open();
                winner
            };
            let (result, winner) = tokio::join!(waiting, publish);
            assert!(matches!(result, Err(Error::Conflict(_))));
            let settings = owned.hito.plugins.settings(&owned.actor).await.unwrap();
            assert_eq!(settings.installations.len(), 1);
            assert_eq!(settings.installations[0].id, winner.id);
            assert!(settings.installations[0].credential_set && winner.revision == 1);
            assert!(matches!(
                owned
                    .hito
                    .plugins
                    .install(&owned.actor, owned.install_request(&preview.digest, None),)
                    .await,
                Err(Error::Conflict(_))
            ));
            owned.assert_preserved().await;
            owned.assert_manifests_only(3).await;
        })
        .catch_unwind(),
    )
    .await;
    owned.finish(outcome).await;
}
