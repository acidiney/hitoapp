#![cfg(unix)]
#[path = "api/support.rs"]
#[allow(dead_code)]
mod support;

use hito_core::{CoreConfig, Credentials, FolderWatchSettings, HostDeviceId, Search, WatchMode};
use std::time::Duration;
use tokio::{net::TcpListener, sync::oneshot};

const WAIT: Duration = Duration::from_secs(30);

#[tokio::test]
async fn the_running_server_indexes_a_new_file_by_itself_and_stops_watching_on_shutdown() {
    let directory = tempfile::tempdir().unwrap();
    let music = directory.path().join("music");
    std::fs::create_dir(&music).unwrap();
    support::write_wav(&music.join("first.wav"));
    let mut config = CoreConfig::new(
        directory.path().join("hito.db"),
        music.clone(),
        HostDeviceId::new("watch-runtime").unwrap(),
    );
    config.folder_watching = FolderWatchSettings {
        mode: WatchMode::Poll,
        poll_interval: Duration::from_millis(100),
        debounce: Duration::from_millis(50),
    };
    let core = hito_core::open(config).await.unwrap();
    let session = core
        .identity
        .setup(Credentials {
            username: "owner".into(),
            password: "owned runtime watching password".into(),
        })
        .await
        .unwrap();
    let actor = core.identity.authenticate(&session.token).await.unwrap();
    let library = core.catalog.libraries(&actor).await.unwrap().remove(0).id;
    core.storage.enqueue_scan(&actor, &library).await.unwrap();
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
    let available = || async {
        core.catalog
            .recordings(&actor, Search::default())
            .await
            .unwrap()
            .items
            .iter()
            .filter(|recording| recording.available)
            .count()
    };
    tokio::time::timeout(WAIT, async {
        while available().await != 1 {
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    })
    .await
    .expect("The first scan must index the existing file");
    tokio::time::sleep(Duration::from_millis(500)).await;
    hito_core::test_support::support::write_wav(&music.join("second.wav"), 1);
    tokio::time::timeout(WAIT, async {
        while available().await != 2 {
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    })
    .await
    .expect("The running server must notice the new file without a manual scan");
    stop.send(()).unwrap();
    tokio::time::timeout(Duration::from_secs(10), serving)
        .await
        .expect("Shutdown must stop the folder watchers")
        .unwrap()
        .unwrap();
}
