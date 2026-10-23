use hito_core::{CoreConfig, Credentials, HostDeviceId, Search};
use hito_server::RuntimeConfig;
use std::path::Path;

fn web_assets(directory: &Path) -> std::path::PathBuf {
    let web = directory.join("web");
    std::fs::create_dir_all(web.join("assets")).unwrap();
    std::fs::write(web.join("assets/app.js"), "export {};\n").unwrap();
    web
}

fn audio_fixture(path: &Path) -> Vec<u8> {
    let bytes = hito_testkit::audio::pcm16_wav(8000, 1, 32, |_| 0);
    std::fs::write(path, &bytes).unwrap();
    bytes
}

#[tokio::test]
async fn legacy_audio_remains_playable_after_identity_adoption_and_restart() {
    let directory = tempfile::tempdir().unwrap();
    let database = directory.path().join("hito.db");
    let music = directory.path().join("music");
    std::fs::create_dir(&music).unwrap();
    let expected_audio = audio_fixture(&music.join("Legacy.wav"));
    let web = web_assets(directory.path());
    let legacy = hito_core::open(CoreConfig::new(
        database.clone(),
        music.clone(),
        HostDeviceId::new("server").unwrap(),
    ))
    .await
    .unwrap();
    let session = legacy
        .identity
        .setup(Credentials {
            username: "admin".into(),
            password: "legacy test password".into(),
        })
        .await
        .unwrap();
    let actor = legacy.identity.authenticate(&session.token).await.unwrap();
    let library = legacy.catalog.libraries(&actor).await.unwrap().remove(0).id;
    legacy
        .storage
        .enqueue_scan(&actor, &library)
        .await
        .unwrap()
        .remove(0);
    assert!(legacy.storage.run_next_job().await.unwrap());
    drop(legacy);
    assert!(!database.with_extension("device-id").exists());

    for _ in 0..2 {
        let config = RuntimeConfig::load(database.clone(), music.clone(), web.clone(), None)
            .await
            .unwrap();
        assert_eq!(config.core.host_device.as_str(), "server");
        let services = hito_core::open(config.core).await.unwrap();
        let actor = services
            .identity
            .authenticate(&session.token)
            .await
            .unwrap();
        let catalog = services
            .catalog
            .recordings(&actor, Search::default())
            .await
            .unwrap();
        assert_eq!(catalog.total, 1);
        assert!(catalog.items[0].available);
        let sources = services
            .catalog
            .sources(&actor, &catalog.items[0].id)
            .await
            .unwrap();
        assert!(sources[0].ready);
        let mut path = services
            .playback
            .audio_file(&actor, &sources[0].id)
            .await
            .unwrap();
        let mut actual = Vec::new();
        std::io::Read::read_to_end(&mut path.file, &mut actual).unwrap();
        assert_eq!(actual, expected_audio);
    }
    assert_eq!(
        std::fs::read_to_string(database.with_extension("device-id")).unwrap(),
        "server"
    );
}

#[tokio::test]
async fn fresh_installations_keep_unique_host_ids_after_the_database_is_created() {
    let directory = tempfile::tempdir().unwrap();
    let database = directory.path().join("hito.db");
    let music = directory.path().join("music");
    let web = web_assets(directory.path());
    let first = RuntimeConfig::load(database.clone(), music.clone(), web.clone(), None)
        .await
        .unwrap();
    let initial_device = first.core.host_device.clone();
    uuid::Uuid::parse_str(initial_device.as_str()).unwrap();
    let services = hito_core::open(first.core).await.unwrap();
    assert!(services.healthy().await);
    drop(services);
    let restarted = RuntimeConfig::load(database.clone(), music.clone(), web.clone(), None)
        .await
        .unwrap();
    assert_eq!(restarted.core.host_device, initial_device);
    assert_eq!(
        std::fs::read_to_string(database.with_extension("device-id")).unwrap(),
        initial_device.as_str()
    );

    let separate = RuntimeConfig::load(directory.path().join("separate.db"), music, web, None)
        .await
        .unwrap();
    uuid::Uuid::parse_str(separate.core.host_device.as_str()).unwrap();
    assert_ne!(separate.core.host_device, initial_device);
}

#[tokio::test]
async fn explicit_host_overrides_leave_the_saved_identity_unchanged() {
    let directory = tempfile::tempdir().unwrap();
    let database = directory.path().join("hito.db");
    let music = directory.path().join("music");
    let web = web_assets(directory.path());
    let first = RuntimeConfig::load(database.clone(), music.clone(), web.clone(), None)
        .await
        .unwrap();
    let saved_device = first.core.host_device;
    let overridden = RuntimeConfig::load(
        database.clone(),
        music.clone(),
        web.clone(),
        Some(HostDeviceId::new("explicit-host").unwrap()),
    )
    .await
    .unwrap();
    assert_eq!(overridden.core.host_device.as_str(), "explicit-host");
    assert_eq!(
        std::fs::read_to_string(database.with_extension("device-id")).unwrap(),
        saved_device.as_str()
    );
    let restored = RuntimeConfig::load(database, music, web, None)
        .await
        .unwrap();
    assert_eq!(restored.core.host_device, saved_device);
}
