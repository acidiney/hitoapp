use hito_core::test_support::content_service as fixture;
use hito_core::test_support::support;
use hito_core::{ConfiguredOffer, ConfiguredPluginService, Error};
use serde_json::json;

fn parse(value: &str) -> Vec<ConfiguredPluginService> {
    ConfiguredPluginService::parse_list(value).unwrap()
}

#[test]
fn a_service_list_accepts_labelled_http_addresses_and_ignores_blank_entries() {
    assert!(parse("").is_empty());
    assert!(parse("  ,, ").is_empty());
    let services = parse("spotify=http://plugin-spotify:8092, youtube=http://plugin-youtube:8093/");
    assert_eq!(services.len(), 2);
    assert_eq!(services[0].label(), "spotify");
    assert_eq!(services[0].base_url(), "http://plugin-spotify:8092/");
    assert_eq!(services[1].base_url(), "http://plugin-youtube:8093/");
}

#[test]
fn a_service_list_refuses_what_an_administrator_offer_would_refuse() {
    for refused in [
        "spotify",
        "=http://plugin-spotify:8092/",
        "Spotify=http://plugin-spotify:8092/",
        "spot ify=http://plugin-spotify:8092/",
        "spotify=ftp://plugin-spotify/",
        "spotify=http://user:secret@plugin-spotify:8092/",
        "spotify=http://plugin-spotify:8092/?token=1",
        "spotify=http://plugin-spotify:8092/#fragment",
        "spotify=not a url",
        "spotify=http://a:1/,spotify=http://b:1/",
        "one=http://a:1/,two=http://a:1",
        &format!("{}=http://a:1/", "x".repeat(33)),
    ] {
        assert!(
            ConfiguredPluginService::parse_list(refused).is_err(),
            "{refused}"
        );
    }
    let many = (0..17)
        .map(|index| format!("s{index}=http://plugin-{index}:80/"))
        .collect::<Vec<_>>()
        .join(",");
    assert!(ConfiguredPluginService::parse_list(&many).is_err());
}

#[tokio::test]
async fn a_configured_service_is_offered_once_with_its_manifest_name_and_connects_nobody() {
    let provider = fixture::ProviderServer::start().await;
    let (_directory, hito, actor, _library, _music) = support::library("configured-offer").await;
    let service = parse(&format!("fixture={}", provider.url)).remove(0);

    assert_eq!(
        hito.plugins.offer_configured(&service).await.unwrap(),
        ConfiguredOffer::Offered
    );
    assert_eq!(
        hito.plugins.offer_configured(&service).await.unwrap(),
        ConfiguredOffer::AlreadyKnown
    );

    let settings = hito.plugins.settings(&actor).await.unwrap();
    assert_eq!(settings.services.len(), 1);
    assert_eq!(settings.services[0].name, "Content fixture");
    assert_eq!(settings.services[0].base_url, service.base_url());
    assert!(settings.installations.is_empty());
    provider.shutdown().await;
}

#[tokio::test]
async fn a_service_the_administrator_withdrew_is_not_offered_again() {
    let provider = fixture::ProviderServer::start().await;
    let (_directory, hito, actor, _library, _music) =
        support::library("configured-withdrawn").await;
    let service = parse(&format!("fixture={}", provider.url)).remove(0);
    hito.plugins.offer_configured(&service).await.unwrap();
    let offered = hito
        .plugins
        .settings(&actor)
        .await
        .unwrap()
        .services
        .remove(0);
    hito.plugins
        .withdraw_service(&actor, &offered.id, offered.revision)
        .await
        .unwrap();

    assert_eq!(
        hito.plugins.offer_configured(&service).await.unwrap(),
        ConfiguredOffer::AlreadyKnown
    );
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

#[tokio::test]
async fn an_unreachable_or_invalid_service_is_not_offered_and_can_be_retried() {
    let provider = fixture::ProviderServer::start().await;
    let (_directory, hito, actor, _library, _music) = support::library("configured-refused").await;
    let service = parse(&format!("fixture={}", provider.url)).remove(0);

    provider.offline().await;
    assert!(matches!(
        hito.plugins.offer_configured(&service).await,
        Err(Error::Upstream(_))
    ));
    provider.online().await;
    provider
        .reply(
            "v1/manifest",
            json!({"id": "", "name": "Broken", "protocol_version": 1, "capabilities": []}),
        )
        .await;
    assert!(hito.plugins.offer_configured(&service).await.is_err());
    assert!(
        hito.plugins
            .settings(&actor)
            .await
            .unwrap()
            .services
            .is_empty()
    );

    provider
        .reply(
            "v1/manifest",
            json!({"id":"org.hito.content-fixture","name":"Content fixture","protocol_version":1,"capabilities":["search"]}),
        )
        .await;
    assert_eq!(
        hito.plugins.offer_configured(&service).await.unwrap(),
        ConfiguredOffer::Offered
    );
    provider.shutdown().await;
}
