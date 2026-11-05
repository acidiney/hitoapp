use hito_core::ConfiguredPluginService;
use hito_core::test_support::content_service as fixture;
use hito_core::test_support::support;
use hito_server::{RuntimeConfig, offer_configured_plugin_services};
use std::time::Duration;
use tokio::sync::watch;

#[tokio::test]
async fn services_from_the_environment_are_offered_once_a_late_addon_answers() {
    let provider = fixture::ProviderServer::start().await;
    provider.offline().await;
    let (_directory, hito, actor, _library, _music) = support::library("configured-runtime").await;
    let services =
        ConfiguredPluginService::parse_list(&format!("fixture={}", provider.url)).unwrap();
    let (stop, stopped) = watch::channel(false);
    let worker = tokio::spawn(offer_configured_plugin_services(
        hito.plugins.clone(),
        services,
        stopped,
    ));

    tokio::time::sleep(Duration::from_millis(300)).await;
    assert!(
        hito.plugins
            .settings(&actor)
            .await
            .unwrap()
            .services
            .is_empty()
    );
    provider.online().await;
    let offered = tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            let settings = hito.plugins.settings(&actor).await.unwrap();
            if let Some(service) = settings.services.first() {
                return (settings.services.len(), service.name.clone());
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    })
    .await
    .unwrap();
    assert_eq!(offered, (1, "Content fixture".to_string()));
    tokio::time::timeout(Duration::from_secs(5), worker)
        .await
        .unwrap()
        .unwrap();
    drop(stop);
    provider.shutdown().await;
}

#[tokio::test]
async fn a_stopped_worker_returns_while_its_addon_is_still_down() {
    let provider = fixture::ProviderServer::start().await;
    provider.offline().await;
    let (_directory, hito, actor, _library, _music) = support::library("configured-stop").await;
    let services =
        ConfiguredPluginService::parse_list(&format!("fixture={}", provider.url)).unwrap();
    let (stop, stopped) = watch::channel(false);
    let worker = tokio::spawn(offer_configured_plugin_services(
        hito.plugins.clone(),
        services,
        stopped,
    ));
    tokio::time::sleep(Duration::from_millis(200)).await;
    stop.send(true).unwrap();
    tokio::time::timeout(Duration::from_secs(5), worker)
        .await
        .unwrap()
        .unwrap();
    assert!(
        hito.plugins
            .settings(&actor)
            .await
            .unwrap()
            .services
            .is_empty()
    );
    provider.shutdown().await;
}

const ENVIRONMENT_TEST: &str = "the_environment_list_is_validated_like_an_administrator_offer";

#[tokio::test]
async fn the_environment_list_is_validated_like_an_administrator_offer() {
    if let Ok(expected) = std::env::var("HITO_PLUGIN_SERVICES_TEST_CHILD") {
        let config = RuntimeConfig::from_env().await;
        match expected.parse::<usize>() {
            Ok(count) => assert_eq!(config.unwrap().plugin_services.len(), count),
            Err(_) => assert!(config.is_err()),
        }
        return;
    }
    let directory = tempfile::tempdir().unwrap();
    let web = directory.path().join("web");
    std::fs::create_dir_all(web.join("assets")).unwrap();
    std::fs::write(web.join("assets/app.js"), "export {};\n").unwrap();
    for (index, (value, expected)) in [
        (None, "0"),
        (Some(""), "0"),
        (
            Some("spotify=http://plugin-spotify:8092/,youtube=http://plugin-youtube:8093/"),
            "2",
        ),
        (
            Some("spotify=http://user:pw@plugin-spotify:8092/"),
            "refused",
        ),
        (Some("spotify"), "refused"),
    ]
    .into_iter()
    .enumerate()
    {
        let mut command = tokio::process::Command::new(std::env::current_exe().unwrap());
        command
            .args(["--exact", ENVIRONMENT_TEST, "--nocapture"])
            .env_clear()
            .env("HITO_PLUGIN_SERVICES_TEST_CHILD", expected)
            .env(
                "HITO_DATABASE",
                directory.path().join(format!("owned-{index}.db")),
            )
            .env("HITO_MUSIC_ROOT", directory.path().join("music"))
            .env("HITO_WEB_DIR", &web)
            .env("HITO_DEVICE_ID", "owned-plugin-services-host")
            .kill_on_drop(true);
        if let Some(value) = value {
            command.env("HITO_PLUGIN_SERVICES", value);
        }
        let output = tokio::time::timeout(Duration::from_secs(10), command.output())
            .await
            .unwrap()
            .unwrap();
        assert!(output.status.success(), "case {index}");
    }
}
