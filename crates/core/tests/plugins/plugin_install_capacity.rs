use hito_core::test_support::content_service as fixture;
#[path = "../fixtures/plugin_installation.rs"]
mod installation;
use hito_core::test_support::support;

use futures_util::FutureExt;
use hito_core::{CoreConfig, Error, HostDeviceId, InstallPlugin, OfferPluginService};
use installation::InstallationFixture;
use std::{panic::AssertUnwindSafe, time::Duration};

#[tokio::test]
async fn a_held_preview_rechecks_account_capacity_after_another_service_is_installed() {
    capacity_case(false).await;
}

#[tokio::test]
async fn a_held_accepted_installation_rechecks_account_capacity_before_publication() {
    capacity_case(true).await;
}

async fn capacity_case(accept: bool) {
    let owned = tokio::time::timeout(Duration::from_secs(10), InstallationFixture::start())
        .await
        .expect("the owned fixture must start within its deadline");
    let mut providers = Vec::new();
    let outcome = tokio::time::timeout(
        Duration::from_secs(30),
        AssertUnwindSafe(async {
            let mut last_service = None;
            for index in 0..32 {
                providers.push(fixture::ProviderServer::start().await);
                let service = owned
                    .hito
                    .plugins
                    .offer_service(
                        &owned.actor,
                        OfferPluginService {
                            name: format!("Owned capacity service {index}"),
                            base_url: providers.last().unwrap().url.clone(),
                        },
                    )
                    .await
                    .unwrap();
                if index == 31 {
                    last_service = Some(service);
                } else {
                    owned
                        .hito
                        .plugins
                        .install(
                            &owned.actor,
                            InstallPlugin {
                                service_id: service.id,
                                service_revision: service.revision,
                                bearer_token: None,
                                accept_manifest_digest: None,
                            },
                        )
                        .await
                        .unwrap();
                }
            }
            let previous = owned.hito.plugins.settings(&owned.actor).await.unwrap();
            assert_eq!(previous.installations.len(), 31);
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
            let service = last_service.unwrap();
            let gate = owned.server.hold("v1/manifest").await;
            let waiting = async {
                if accept {
                    owned
                        .hito
                        .plugins
                        .install(&owned.actor, owned.install_request(&preview.digest, None))
                        .await
                        .map(|_| ())
                } else {
                    owned
                        .hito
                        .plugins
                        .preview_installation(&owned.actor, owned.preview_request(None))
                        .await
                        .map(|_| ())
                }
            };
            let fill = async {
                tokio::time::timeout(Duration::from_secs(3), gate.entered())
                    .await
                    .expect("the target manifest must reach its owned barrier");
                let winner = concurrent
                    .plugins
                    .install(
                        &owned.actor,
                        InstallPlugin {
                            service_id: service.id,
                            service_revision: service.revision,
                            bearer_token: None,
                            accept_manifest_digest: None,
                        },
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
            let (result, winner) = tokio::join!(waiting, fill);
            assert!(matches!(result, Err(Error::Invalid(_))));
            let settings = owned.hito.plugins.settings(&owned.actor).await.unwrap();
            assert_eq!(settings.installations.len(), 32);
            assert!(
                settings
                    .installations
                    .iter()
                    .any(|item| item.id == winner.id)
            );
            assert!(
                !settings
                    .installations
                    .iter()
                    .any(|item| item.service_id == owned.service.id)
            );
            assert!(previous.installations.iter().all(|previous| {
                settings.installations.iter().any(|item| {
                    serde_json::to_value(item).unwrap() == serde_json::to_value(previous).unwrap()
                })
            }));
            assert!(matches!(
                owned
                    .hito
                    .plugins
                    .preview_installation(&owned.actor, owned.preview_request(None),)
                    .await,
                Err(Error::Invalid(_))
            ));
            assert!(matches!(
                owned
                    .hito
                    .plugins
                    .install(&owned.actor, owned.install_request(&preview.digest, None),)
                    .await,
                Err(Error::Invalid(_))
            ));
            owned.assert_preserved().await;
            owned.assert_manifests_only(2).await;
            for provider in &providers {
                assert_eq!(provider.requests("v1/manifest").await.len(), 1);
                assert!(provider.requests("v1/audio").await.is_empty());
            }
        })
        .catch_unwind(),
    )
    .await;
    let mut stopped = true;
    for provider in providers {
        stopped &= tokio::time::timeout(Duration::from_secs(2), provider.shutdown())
            .await
            .is_ok();
    }
    owned.finish(outcome).await;
    assert!(stopped, "every owned capacity provider must stop and join");
}
