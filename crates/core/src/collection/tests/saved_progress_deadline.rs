use crate::collection::saved_progress;
use crate::{CoreConfig, Credentials, Error, HostDeviceId, open};
use std::time::Duration;
use uuid::Uuid;

#[tokio::test]
async fn a_contended_saved_progress_read_fails_as_upstream_within_its_deadline() {
    let directory = tempfile::tempdir().unwrap();
    let music = directory.path().join("music");
    std::fs::create_dir(&music).unwrap();
    let hito = open(CoreConfig::new(
        directory.path().join("hito.db"),
        music,
        HostDeviceId::new("saved-progress-deadline").unwrap(),
    ))
    .await
    .unwrap();
    let session = hito
        .identity
        .setup(Credentials {
            username: "deadline-owner".into(),
            password: "owned saved progress deadline password".into(),
        })
        .await
        .unwrap();
    let owner = hito.identity.authenticate(&session.token).await.unwrap();
    let store = hito.collection.store.clone();
    let mut held = Vec::new();
    for _ in 0..store.pool.options().get_max_connections() {
        held.push(store.pool.acquire().await.unwrap());
    }

    let result = tokio::time::timeout(
        Duration::from_secs(5),
        saved_progress(&store, &owner, &[Uuid::new_v4()]),
    )
    .await
    .expect("the read must give up inside its own deadline");
    assert!(matches!(
        result,
        Err(Error::Upstream(
            "Saved playback could not be checked in time"
        ))
    ));
    drop(held);
}
