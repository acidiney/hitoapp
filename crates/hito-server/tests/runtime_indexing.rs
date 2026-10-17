#![cfg(unix)]
#[path = "../../hito-core/tests/fixtures/media_tool.rs"]
mod media_tool;
#[path = "api/support.rs"]
#[allow(dead_code)]
mod support;

use hito_core::{CoreConfig, Credentials, HostDeviceId};
use std::{path::Path, time::Duration};
use tokio::{net::TcpListener, sync::oneshot};

async fn exercise(abort: bool) {
    let directory = tempfile::tempdir().unwrap();
    let music = directory.path().join("music");
    std::fs::create_dir(&music).unwrap();
    support::write_wav(&music.join("fixture.wav"));
    let marker = directory.path().join("inspection");
    let mut config = CoreConfig::new(
        directory.path().join("hito.db"),
        music,
        HostDeviceId::new("runtime-test").unwrap(),
    );
    config.media_tools.ffprobe = media_tool::script(
        directory.path(),
        "ffprobe",
        &format!(
            "previous=''\nfor argument do\n  if [ \"$previous\" = '-i' ]; then input=$argument; fi\n  previous=$argument\ndone\nprintf '%s\\n%s\\n' \"$$\" \"$input\" > {}\nexec sleep 300",
            media_tool::quote(&marker)
        ),
    );
    let core = hito_core::open(config).await.unwrap();
    let session = core
        .identity
        .setup(Credentials {
            username: "owner".into(),
            password: "owned runtime indexing password".into(),
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
    let (pid, input) = tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            if let Ok(contents) = std::fs::read_to_string(&marker) {
                let lines = contents.lines().collect::<Vec<_>>();
                if lines.len() == 2 {
                    break (lines[0].to_owned(), lines[1].to_owned());
                }
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("The worker must already be inspecting owned audio");
    assert!(Path::new(&input).exists());
    if abort {
        serving.abort();
        assert!(serving.await.unwrap_err().is_cancelled());
    } else {
        stop.send(()).unwrap();
        tokio::time::timeout(Duration::from_secs(3), serving)
            .await
            .expect("Shutdown must cancel the active scan")
            .unwrap()
            .unwrap();
    }
    if !abort {
        assert!(
            !std::process::Command::new("kill")
                .args(["-0", &pid])
                .stderr(std::process::Stdio::null())
                .status()
                .unwrap()
                .success(),
            "A completed host shutdown must already have reaped its inspection process"
        );
        assert!(
            !Path::new(&input).exists(),
            "The inspection input must be released before shutdown returns"
        );
    }
    tokio::time::timeout(Duration::from_secs(3), async {
        while std::process::Command::new("kill")
            .args(["-0", &pid])
            .stderr(std::process::Stdio::null())
            .status()
            .unwrap()
            .success()
            || Path::new(&input).exists()
        {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("The inspection process and private input must be released");
    assert_eq!(
        core.catalog
            .recordings(&actor, hito_core::Search::default())
            .await
            .unwrap()
            .total,
        0
    );
}

#[tokio::test]
async fn shutdown_cancels_and_reaps_an_active_inspection() {
    exercise(false).await;
}

#[tokio::test]
async fn cancelling_the_host_stops_an_already_running_scan() {
    exercise(true).await;
}
