#![cfg(unix)]
use hito_core::{CoreConfig, Credentials, HostDeviceId};
use std::time::Duration;
use tokio::{net::TcpListener, sync::oneshot};

#[tokio::test]
async fn starting_the_server_gives_an_account_that_never_had_a_matching_walk_its_first_one() {
    let directory = tempfile::tempdir().unwrap();
    let music = directory.path().join("music");
    std::fs::create_dir(&music).unwrap();
    let config = CoreConfig::new(
        directory.path().join("hito.db"),
        music,
        HostDeviceId::new("runtime-test").unwrap(),
    );
    let core = hito_core::open(config).await.unwrap();
    let session = core
        .identity
        .setup(Credentials {
            username: "owner".into(),
            password: "owned first matching walk password".into(),
        })
        .await
        .unwrap();
    let actor = core.identity.authenticate(&session.token).await.unwrap();
    let before = core
        .artist_catalog
        .artist_matching_status(&actor)
        .await
        .unwrap();
    assert!(before.last_finished_at.is_none());

    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let (stop, stopped) = oneshot::channel();
    let serving = tokio::spawn(hito_server::serve(
        listener,
        hito_server::router(core.clone()),
        core.clone(),
        async {
            let _ = stopped.await;
        },
    ));
    tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            let status = core
                .artist_catalog
                .artist_matching_status(&actor)
                .await
                .unwrap();
            if status.last_finished_at.is_some() {
                break;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    })
    .await
    .expect("The first walk must be queued and finished by the worker at start");
    stop.send(()).unwrap();
    serving.await.unwrap().unwrap();
}
