use super::fixture::{Copies, HOST, drain, drain_owned, managed_files, sql};
use hito_core::{CoreConfig, HostDeviceId, test_support::hang_guard::hang_guard};
use std::time::Duration;

#[tokio::test]
async fn a_copy_interrupted_by_a_restart_is_fetched_again_and_published() {
    let f = Copies::start().await;
    let next = f.recording("next").await;
    let gate = f.server.hold("audio-tail").await;
    f.announce(&f.actor, &[&next]).await;
    let downloads = f.hito.downloads.clone();
    let worker = tokio::spawn(async move { drain_owned(downloads).await });
    tokio::time::timeout(Duration::from_secs(10), gate.entered())
        .await
        .expect("the prefetch must be mid-body");
    worker.abort();
    assert!(worker.await.is_err());
    tokio::time::timeout(Duration::from_secs(5), f.hito.downloads.finish_downloads())
        .await
        .unwrap()
        .unwrap();
    let Copies {
        directory,
        music,
        server,
        hito,
        actor,
        ..
    } = f;
    drop(hito);
    gate.open();
    assert_eq!(
        sql(
            &directory,
            "UPDATE download_jobs SET lease_until=unixepoch()-1 WHERE status='running'"
        )
        .await,
        1
    );

    let hito = hang_guard(
        "the restarted core must open",
        hito_core::open(CoreConfig::new(
            directory.path().join("hito.db"),
            music,
            HostDeviceId::new(HOST).unwrap(),
        )),
    )
    .await
    .unwrap();
    assert!(drain(&hito).await > 0);
    let copy = hito
        .catalog
        .sources(&actor, &next)
        .await
        .unwrap()
        .into_iter()
        .find(|source| source.provider == "download" && source.ready);
    assert!(copy.is_some());
    let names = managed_files(&directory);
    assert_eq!(names.len(), 1, "{names:?}");
    assert!(names[0].ends_with(".media"));
    assert_eq!(server.requests("v1/audio").await.len(), 2);
}
