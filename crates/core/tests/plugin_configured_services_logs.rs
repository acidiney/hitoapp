use hito_core::ConfiguredPluginService;
use hito_core::test_support::content_service as fixture;
use hito_core::test_support::metadata_tag_logs as logs;
use hito_core::test_support::support;

#[tokio::test(flavor = "current_thread")]
async fn offering_configured_services_logs_outcomes_but_never_addresses_or_names() {
    let provider = fixture::ProviderServer::start().await;
    let (_directory, hito, _actor, _library, _music) = support::library("configured-logs").await;
    let captured = logs::CapturedLogs::default();
    tracing::subscriber::set_global_default(captured.subscriber()).unwrap();
    let reachable =
        ConfiguredPluginService::parse_list(&format!("fixture={}", provider.url)).unwrap();
    let unreachable =
        ConfiguredPluginService::parse_list("gone=http://private-host-sentinel.invalid:9/")
            .unwrap();

    hito.plugins.offer_configured(&reachable[0]).await.unwrap();
    hito.plugins.offer_configured(&reachable[0]).await.unwrap();
    assert!(
        hito.plugins
            .offer_configured(&unreachable[0])
            .await
            .is_err()
    );

    let text = captured.text();
    assert!(text.contains("Configured plugin service offered"), "{text}");
    assert!(text.contains("\"outcome\":\"offered\""), "{text}");
    assert!(text.contains("\"outcome\":\"already_known\""), "{text}");
    assert!(
        text.contains("Configured plugin service not offered"),
        "{text}"
    );
    assert!(text.contains("\"outcome\":\"upstream\""), "{text}");
    assert!(text.contains("\"label\":\"fixture\""), "{text}");
    assert!(!text.contains("private-host-sentinel"), "{text}");
    assert!(!text.contains(&provider.url), "{text}");
    assert!(!text.contains("Content fixture"), "{text}");
    provider.shutdown().await;
}
