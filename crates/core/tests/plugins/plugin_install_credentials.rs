use hito_core::test_support::content_service as fixture;
#[path = "../fixtures/plugin_installation.rs"]
mod installation;
use hito_core::test_support::support;

use futures_util::FutureExt;
use hito_core::{CoreConfig, Error, HostDeviceId, PluginRefresh, RefreshPlugin};
use installation::InstallationFixture;
use std::{panic::AssertUnwindSafe, time::Duration};

const PREVIEW_TOKEN: &str = "private-preview-original-credential";
const ACCEPT_TOKEN: &str = "private-preview-accepted-credential";
const CHANGED_TOKEN: &str = "private-preview-rejected-credential";

#[tokio::test]
async fn acceptance_uses_the_current_credential_for_the_exact_manifest_and_retains_only_that_credential()
 {
    let owned = tokio::time::timeout(Duration::from_secs(10), InstallationFixture::start())
        .await
        .expect("the owned fixture must start within its deadline");
    let outcome = tokio::time::timeout(
        Duration::from_secs(20),
        AssertUnwindSafe(async {
            let preview = owned
                .hito
                .plugins
                .preview_installation(&owned.actor, owned.preview_request(Some(PREVIEW_TOKEN)))
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
            let installed = owned
                .hito
                .plugins
                .install(
                    &owned.actor,
                    owned.install_request(&preview.digest, Some(ACCEPT_TOKEN)),
                )
                .await
                .unwrap();
            assert!(installed.credential_set);
            assert!(
                serde_json::to_value(&installed.manifest).unwrap()
                    == serde_json::to_value(&preview.manifest).unwrap()
            );
            let settings =
                serde_json::to_string(&owned.hito.plugins.settings(&owned.actor).await.unwrap())
                    .unwrap();
            for private in [PREVIEW_TOKEN, ACCEPT_TOKEN, CHANGED_TOKEN] {
                assert!(!settings.contains(private));
            }
            let reopened = hito_core::open(CoreConfig::new(
                owned.directory.path().join("hito.db"),
                owned.directory.path().join("music"),
                HostDeviceId::new("server").unwrap(),
            ))
            .await
            .unwrap();
            let PluginRefresh::Updated {
                installation: checked,
            } = reopened
                .plugins
                .refresh(
                    &owned.actor,
                    &installed.id,
                    RefreshPlugin {
                        revision: installed.revision,
                        accept_manifest_digest: None,
                        credential: Default::default(),
                    },
                )
                .await
                .unwrap()
            else {
                panic!("the exact accepted manifest must remain unchanged after restart");
            };
            assert_eq!(checked.id, installed.id);
            assert!(checked.credential_set && checked.revision == installed.revision + 1);
            for path in ["hito.db", "hito.db-wal"] {
                if let Ok(bytes) = std::fs::read(owned.directory.path().join(path)) {
                    for private in [PREVIEW_TOKEN, ACCEPT_TOKEN, CHANGED_TOKEN] {
                        assert!(
                            !bytes
                                .windows(private.len())
                                .any(|part| part == private.as_bytes()),
                            "private database files must not retain plaintext credentials"
                        );
                    }
                }
            }
            reopened
                .plugins
                .remove(&owned.actor, &checked.id, checked.revision)
                .await
                .unwrap();
            let next = owned
                .hito
                .plugins
                .preview_installation(&owned.actor, owned.preview_request(Some(PREVIEW_TOKEN)))
                .await
                .unwrap();
            assert_eq!(next.digest, preview.digest);
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
            let mut changed = serde_json::to_value(next.manifest).unwrap();
            changed["content_policy"]["source_persistence"] = "playback_only".into();
            owned.server.reply("v1/manifest", changed).await;
            assert!(matches!(
                owned
                    .hito
                    .plugins
                    .install(
                        &owned.actor,
                        owned.install_request(&next.digest, Some(CHANGED_TOKEN)),
                    )
                    .await,
                Err(Error::Conflict(_))
            ));
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
            let requests = owned.server.requests("v1/manifest").await;
            assert_eq!(requests.len(), 5);
            for ((headers, _), token) in requests.iter().zip([
                PREVIEW_TOKEN,
                ACCEPT_TOKEN,
                ACCEPT_TOKEN,
                PREVIEW_TOKEN,
                CHANGED_TOKEN,
            ]) {
                let expected = format!("Bearer {token}");
                assert!(
                    headers
                        .get("authorization")
                        .and_then(|value| value.to_str().ok())
                        == Some(expected.as_str())
                );
                assert!(headers.get("cookie").is_none());
                assert_eq!(headers["x-hito-plugin-protocol"], "1");
            }
            tokio::time::timeout(
                Duration::from_secs(5),
                reopened.downloads.finish_downloads(),
            )
            .await
            .expect("the reopened capability must settle")
            .unwrap();
            owned.assert_preserved().await;
            owned.assert_manifests_only(5).await;
        })
        .catch_unwind(),
    )
    .await;
    owned.finish(outcome).await;
}
