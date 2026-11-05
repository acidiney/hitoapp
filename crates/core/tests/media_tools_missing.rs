use hito_core::{CoreConfig, Credentials, HostDeviceId, JobFailure, open};

async fn scan_with_tools(ffprobe: std::path::PathBuf) -> hito_core::Job {
    let dir = tempfile::tempdir().unwrap();
    let music = dir.path().join("music");
    std::fs::create_dir(&music).unwrap();
    let mut config = CoreConfig::new(
        dir.path().join("hito.db"),
        music,
        HostDeviceId::new("desktop").unwrap(),
    );
    config.media_tools.ffprobe = ffprobe;
    let services = open(config).await.unwrap();
    let session = services
        .identity
        .setup(Credentials {
            username: "admin".into(),
            password: "media tools missing password".into(),
        })
        .await
        .unwrap();
    let actor = services
        .identity
        .authenticate(&session.token)
        .await
        .unwrap();
    let library = services
        .catalog
        .libraries(&actor)
        .await
        .unwrap()
        .remove(0)
        .id;
    let job = services
        .storage
        .enqueue_scan(&actor, &library)
        .await
        .unwrap()
        .remove(0);
    assert!(services.storage.run_next_job().await.unwrap());
    services.storage.job(&actor, &job.id).await.unwrap()
}

#[tokio::test]
async fn a_scan_without_the_media_tools_reports_that_they_are_not_installed() {
    let dir = tempfile::tempdir().unwrap();
    let job = scan_with_tools(dir.path().join("missing-ffprobe")).await;
    assert_eq!(job.state, "failed");
    assert_eq!(job.failure, Some(JobFailure::MediaToolsMissing));
    let message = job.error.unwrap();
    assert!(message.contains("FFmpeg is not installed"), "{message}");
    assert!(message.contains("brew install ffmpeg"), "{message}");
}

#[tokio::test]
async fn a_scan_with_working_tools_and_no_music_completes() {
    let job = scan_with_tools("ffprobe".into()).await;
    assert_eq!(job.state, "complete");
    assert_eq!(job.failure, None);
}
