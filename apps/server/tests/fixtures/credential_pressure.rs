use axum::http::StatusCode;
use hito_plugin_sdk::{Capability, HEALTH_PATH, HealthReport, Manifest, PROTOCOL_VERSION};
use hito_testkit::fake_plugin::{FakePlugin, Reply};
use serde_json::{Value, json};

pub(crate) use crate::source_credential_fixture::{
    Context, OWNER_TOKEN, ProviderServer, Response, account, installation_by_id,
};

pub struct Peer {
    pub account: account::Account,
    pub expected: Value,
}

pub async fn prepare(context: &Context, plugins: &mut Vec<FakePlugin>) -> Vec<Peer> {
    let mut services = Vec::new();
    let settings = context.owner.get(&context.app, "/api/v1/plugins").await;
    services.push(
        settings["services"]
            .as_array()
            .unwrap()
            .iter()
            .find(|service| service["id"] == context.installation["service_id"])
            .unwrap()
            .clone(),
    );
    for index in 0..31 {
        let plugin = FakePlugin::builder(Manifest {
            id: format!("org.hito.credential-pressure-{index}"),
            name: format!("Owned credential pressure {index}"),
            protocol_version: PROTOCOL_VERSION,
            capabilities: [Capability::Search, Capability::Metadata, Capability::Lyrics].into(),
            authorization: None,
            content_policy: None,
        })
        .require_protocol()
        .get(HEALTH_PATH, |_| {
            Reply::Json(serde_json::to_value(HealthReport::ok()).unwrap())
        })
        .start()
        .await;
        plugins.push(plugin);
        let (status, service) = context.owner.request(&context.app, "POST", "/api/v1/plugins/services",
            json!({"name":format!("Owned pressure {index}"), "base_url":plugins.last().unwrap().url})).await;
        assert_eq!(status, StatusCode::CREATED);
        services.push(service);
    }
    assert_eq!(services.len(), 32);
    let mut peers = Vec::new();
    for index in 0..4 {
        let account = if index == 0 {
            account::Account::login(
                &context.app,
                "source-lifecycle-peer",
                "owned source lifecycle peer password",
            )
            .await
        } else {
            let username = format!("credential-pressure-peer-{index}");
            let password = format!("owned credential pressure peer {index} password");
            let (status, _) = context
                .owner
                .request(
                    &context.app,
                    "POST",
                    "/api/v1/users",
                    json!({"username":username, "password":password}),
                )
                .await;
            assert_eq!(status, StatusCode::CREATED);
            account::Account::login(&context.app, &username, &password).await
        };
        for (position, service) in services.iter().enumerate() {
            if index == 0 && position == 0 {
                continue;
            }
            let (status, _) = account
                .request(
                    &context.app,
                    "POST",
                    "/api/v1/plugins",
                    json!({
                        "service_id":service["id"], "service_revision":service["revision"],
                        "bearer_token":null,
                    }),
                )
                .await;
            assert_eq!(status, StatusCode::CREATED);
        }
        let mut expected = settings_for(context, &account).await;
        assert_eq!(expected["installations"].as_array().unwrap().len(), 32);
        for installation in expected["installations"].as_array_mut().unwrap() {
            assert_eq!(installation["health"]["state"], "ok");
            let (status, cleared) = account
                .request(
                    &context.app,
                    "DELETE",
                    &format!(
                        "/api/v1/plugins/{}/credential",
                        installation["id"].as_str().unwrap()
                    ),
                    json!({"revision":installation["revision"]}),
                )
                .await;
            assert_eq!(status, StatusCode::OK);
            assert_eq!(cleared["id"], installation["id"]);
            assert_eq!(cleared["service_id"], installation["service_id"]);
            assert_eq!(cleared["manifest"], installation["manifest"]);
            assert_eq!(cleared["checked_at"], installation["checked_at"]);
            assert_eq!(
                cleared["revision"].as_u64().unwrap(),
                installation["revision"].as_u64().unwrap() + 1
            );
            assert_eq!(cleared["credential_set"], false);
            *installation = cleared;
            installation["health"] =
                json!({"state":"credentials_required", "reason":null, "since":null});
        }
        peers.push(Peer { account, expected });
    }
    assert_eq!(peers.len(), 4);
    peers
}

pub async fn settings_for(context: &Context, account: &account::Account) -> Value {
    let (status, headers, settings) = account::exchange(
        &context.app,
        "GET",
        "/api/v1/plugins",
        Value::Null,
        account.headers(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(headers["cache-control"], "private, no-store");
    settings
}
