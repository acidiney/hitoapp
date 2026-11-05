use hito_core::test_support::content_service as fixture;
use hito_core::test_support::support;
use hito_core::{
    Actor, InstallPlugin, OfferPluginService, PluginAuthorization, PluginAuthorizationCallback,
    PluginAuthorizationOutcome, PluginAuthorizationRedirect, PluginAuthorizationState, Services,
};
use serde_json::json;

const FIXED: &str = "http://127.0.0.1:47321/api/v1/plugin-authorization/callback";

#[test]
fn desktop_loopback_accepts_only_a_literal_loopback_address_with_a_port_and_the_callback_path() {
    for accepted in [
        FIXED,
        "http://[::1]:47321/api/v1/plugin-authorization/callback",
    ] {
        assert!(PluginAuthorizationRedirect::desktop_loopback(accepted).is_ok());
    }
    for refused in [
        "https://127.0.0.1:47321/api/v1/plugin-authorization/callback",
        "http://127.0.0.1/api/v1/plugin-authorization/callback",
        "http://localhost:47321/api/v1/plugin-authorization/callback",
        "http://192.168.1.5:47321/api/v1/plugin-authorization/callback",
        "https://hito.example/api/v1/plugin-authorization/callback",
        "http://127.0.0.1:47321/other",
        "http://127.0.0.1:47321/api/v1/plugin-authorization/callback?x=1",
        "http://user@127.0.0.1:47321/api/v1/plugin-authorization/callback",
    ] {
        assert!(
            PluginAuthorizationRedirect::desktop_loopback(refused).is_err(),
            "{refused}"
        );
    }
}

struct Fixture {
    hito: Services,
    actor: Actor,
    installation: String,
    revision: i64,
    _provider: fixture::ProviderServer,
    _directory: tempfile::TempDir,
}

async fn fixture() -> Fixture {
    let provider = fixture::ProviderServer::start().await;
    provider
        .reply(
            "v1/manifest",
            json!({
                "id": "org.hito.loopback-fixture",
                "name": "Loopback fixture",
                "protocol_version": 1,
                "capabilities": ["playlists"],
                "authorization": {
                    "kind": "oauth2_pkce",
                    "client_id": "owned-test-client",
                    "authorization_endpoint": "https://accounts.spotify.com/authorize",
                    "token_endpoint": "https://accounts.spotify.com/api/token",
                    "scopes": ["playlist-read-private"]
                }
            }),
        )
        .await;
    let (directory, hito, actor, _library, _music) = support::library("loopback-host").await;
    let service = hito
        .plugins
        .offer_service(
            &actor,
            OfferPluginService {
                name: "Loopback fixture".into(),
                base_url: provider.url.clone(),
            },
        )
        .await
        .unwrap();
    let installation = hito
        .plugins
        .install(
            &actor,
            InstallPlugin {
                accept_manifest_digest: None,
                service_id: service.id,
                service_revision: service.revision,
                bearer_token: None,
            },
        )
        .await
        .unwrap();
    Fixture {
        hito,
        actor,
        installation: installation.id,
        revision: installation.revision,
        _provider: provider,
        _directory: directory,
    }
}

fn callback(attempt: &PluginAuthorization) -> PluginAuthorizationCallback {
    let url: url::Url = attempt.authorization_url.parse().unwrap();
    let state = url
        .query_pairs()
        .find(|(key, _)| key == "state")
        .unwrap()
        .1
        .into_owned();
    PluginAuthorizationCallback::parse(Some(&format!("code=owned-code&state={state}"))).unwrap()
}

#[tokio::test]
async fn a_session_attempt_cannot_be_completed_without_its_session_and_a_desktop_attempt_is_not_a_session_attempt()
 {
    let fixture = fixture().await;
    let plugins = &fixture.hito.plugins;
    let browser = plugins
        .begin_authorization(
            &fixture.actor,
            &fixture.installation,
            fixture.revision,
            &PluginAuthorizationRedirect::new(
                "https://hito.test/api/v1/plugin-authorization/callback",
            )
            .unwrap(),
        )
        .await
        .unwrap();
    let outcome = plugins
        .complete_loopback_authorization(callback(&browser))
        .await
        .unwrap();
    assert_eq!(outcome, PluginAuthorizationOutcome::Invalid);
    let status = plugins
        .authorization_status(&fixture.actor, &fixture.installation, &browser.id)
        .await
        .unwrap();
    assert!(matches!(status.status, PluginAuthorizationState::Pending));

    let desktop = plugins
        .begin_authorization(
            &fixture.actor,
            &fixture.installation,
            fixture.revision,
            &PluginAuthorizationRedirect::desktop_loopback(FIXED).unwrap(),
        )
        .await
        .unwrap();
    let outcome = plugins
        .complete_authorization(&fixture.actor, callback(&desktop))
        .await
        .unwrap();
    assert_eq!(outcome, PluginAuthorizationOutcome::Invalid);
    let status = plugins
        .authorization_status(&fixture.actor, &fixture.installation, &desktop.id)
        .await
        .unwrap();
    assert!(matches!(status.status, PluginAuthorizationState::Pending));
}
