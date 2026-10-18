#[path = "fixtures/playlist_read_barrier.rs"]
mod barrier;
#[path = "fixtures/content_service.rs"]
mod fixture;
#[path = "fixtures/plugin_installation.rs"]
mod installation;
mod support;

use futures_util::FutureExt;
use hito_core::{CoreConfig, Error, HostDeviceId};
use installation::InstallationFixture;
use std::{panic::AssertUnwindSafe, time::Duration};
use tracing_subscriber::prelude::*;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_preview_rechecks_live_session_after_its_final_snapshot_is_released() {
    let row = barrier::HeaderBarrier::matching(
        "SELECT id,name,base_url,revision FROM plugin_services",
        "WHERE id = ? AND active = 1",
    );
    tracing::subscriber::set_global_default(tracing_subscriber::registry().with(row.clone()))
        .unwrap();
    let owned = tokio::time::timeout(Duration::from_secs(10), InstallationFixture::start())
        .await
        .expect("the owned fixture must start within its deadline");
    let outcome = tokio::time::timeout(
        Duration::from_secs(15),
        AssertUnwindSafe(async {
            let (actor, session) = owned.session().await;
            let concurrent = hito_core::open(CoreConfig::new(
                owned.directory.path().join("hito.db"),
                owned.directory.path().join("music"),
                HostDeviceId::new("server").unwrap(),
            ))
            .await
            .unwrap();
            let manifest = owned.server.hold("v1/manifest").await;
            let reading = owned
                .hito
                .plugins
                .preview_installation(&actor, owned.preview_request(None));
            let revoke = async {
                tokio::time::timeout(Duration::from_secs(3), manifest.entered())
                    .await
                    .expect("the manifest must reach its owned HTTP barrier");
                let _release_on_drop = row.arm();
                manifest.open();
                tokio::time::timeout(Duration::from_secs(2), row.entered())
                    .await
                    .expect("the final service row must reach its SQL completion barrier");
                tokio::time::timeout(Duration::from_secs(2), concurrent.identity.logout(&session))
                    .await
                    .expect("real logout must commit while the old read snapshot is held")
                    .unwrap();
                row.release();
            };
            let (result, ()) = tokio::join!(reading, revoke);
            assert!(
                !row.timed_out(),
                "the SQL barrier must be released explicitly, not by its deadline"
            );
            assert!(
                matches!(
                    owned.hito.identity.authenticate(&session).await,
                    Err(Error::Unauthorized)
                ),
                "the real session revocation must be committed before checking the preview outcome"
            );
            assert!(
                matches!(result, Err(Error::Unauthorized)),
                "a preview must reject a session revoked after its final authorization snapshot"
            );
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
            owned.assert_manifests_only(1).await;
            tokio::time::timeout(
                Duration::from_secs(5),
                concurrent.downloads.finish_downloads(),
            )
            .await
            .expect("the concurrent capability must settle")
            .unwrap();
        })
        .catch_unwind(),
    )
    .await;
    owned.finish(outcome).await;
}
