use axum::{
    Router,
    http::{HeaderMap, StatusCode},
};
use hito_plugin_sdk::{MANIFEST_PATH, SEARCH_PATH};
use hito_testkit::fake_plugin::FakePlugin;
use serde_json::{Value, json};
use uuid::Uuid;

pub(crate) use crate::account;
pub(crate) use hito_core::test_support::resolution_unavailable as library;
pub use library::ProviderServer;

pub const OWNER_TOKEN: &str = "owned-source-lifecycle-owner-bearer";
pub const PEER_TOKEN: &str = "owned-source-lifecycle-peer-bearer";
pub const NEW_TOKEN: &str = "owned-source-lifecycle-new-bearer";
pub type Response = (StatusCode, HeaderMap, Value);

pub struct Context {
    pub owned: library::SavedLibrary,
    pub app: Router,
    pub owner: account::Account,
    pub installation: Value,
    pub before: Value,
    pub choice: Value,
    peer: account::Account,
    peer_before: Value,
    identities: Value,
}

impl Context {
    pub async fn prepare(
        primary: &ProviderServer,
        previous: &ProviderServer,
        plugin: &FakePlugin,
    ) -> Self {
        let owned = library::SavedLibrary::prepare(primary, previous).await;
        let old = owned
            .hito
            .plugins
            .connections(&owned.actor)
            .await
            .unwrap()
            .installations
            .into_iter()
            .find(|item| item.manifest.id == "org.hito.audio-fixture")
            .unwrap();
        owned
            .hito
            .plugins
            .remove(&owned.actor, &old.id, old.revision)
            .await
            .unwrap();
        let app = hito_server::router(owned.hito.clone());
        let owner = account::Account::login(&app, "admin", "indexing test password").await;
        let installation = owner
            .connect(
                &app,
                &plugin.url,
                "Owned source lifecycle service",
                Some(OWNER_TOKEN),
            )
            .await;
        let before = Self::settings_for(&app, &owner).await;
        let current = installation_by_id(&before, &installation["id"]);
        assert_eq!(current["health"]["state"], "ok");
        assert_eq!(current["credential_set"], true);
        let service = before["services"]
            .as_array()
            .unwrap()
            .iter()
            .find(|item| item["id"] == installation["service_id"])
            .unwrap();
        let (status, _) = owner.request(&app, "POST", "/api/v1/users",
            json!({"username":"source-lifecycle-peer", "password":"owned source lifecycle peer password"})).await;
        assert_eq!(status, StatusCode::CREATED);
        let peer = account::Account::login(
            &app,
            "source-lifecycle-peer",
            "owned source lifecycle peer password",
        )
        .await;
        let (status, peer_installation) = peer.request(&app, "POST", "/api/v1/plugins", json!({
            "service_id": service["id"], "service_revision": service["revision"], "bearer_token": PEER_TOKEN
        })).await;
        assert_eq!(status, StatusCode::CREATED);
        assert_ne!(peer_installation["id"], installation["id"]);
        let peer_before = Self::settings_for(&app, &peer).await;
        assert_eq!(peer_before["installations"].as_array().unwrap().len(), 1);
        assert_eq!(
            installation_by_id(&peer_before, &peer_installation["id"])["health"]["state"],
            "ok"
        );
        let choice = owner
            .get(
                &app,
                &format!("/api/v1/recordings/{}/source-choice", owned.recording),
            )
            .await;
        assert_eq!(choice["mode"], "automatic");
        assert_eq!(choice["revision"], 0);
        let identities = owner
            .get(
                &app,
                &format!("/api/v1/recordings/{}/lyrics/identities", owned.recording),
            )
            .await;
        let context = Self {
            owned,
            app,
            owner,
            installation,
            before,
            choice,
            peer,
            peer_before,
            identities,
        };
        let request = json!({"request_id": Uuid::new_v4()});
        let (status, body) =
            crate::support::call(&context.app, "POST", &context.path(), request.clone(), None)
                .await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);
        assert!(body.get("plugin_connection").is_none());
        let (status, body) = context
            .peer
            .request(&context.app, "POST", &context.path(), request)
            .await;
        assert_eq!(status, StatusCode::NOT_FOUND);
        assert!(body.get("plugin_connection").is_none());
        assert_eq!(plugin.health_requests().len(), 2);
        assert_eq!(plugin.requests_for(MANIFEST_PATH).len(), 2);
        assert!(plugin.requests_for(SEARCH_PATH).is_empty());
        context
    }

    pub fn path(&self) -> String {
        format!(
            "/api/v1/recordings/{}/sources/resolve",
            self.owned.recording
        )
    }

    pub async fn resolve(&self, request: &Value) -> Response {
        account::exchange(
            &self.app,
            "POST",
            &self.path(),
            request.clone(),
            self.owner.headers(),
        )
        .await
    }

    pub async fn settings(&self) -> Value {
        Self::settings_for(&self.app, &self.owner).await
    }

    async fn settings_for(app: &Router, account: &account::Account) -> Value {
        let (status, headers, settings) = account::exchange(
            app,
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

    pub fn check_resolution(&self, response: &Response, request: &Value) {
        assert_eq!(response.0, StatusCode::OK);
        assert_eq!(response.1["cache-control"], "private, no-store");
        assert_eq!(response.2["request_id"], request["request_id"]);
        assert_eq!(response.2["recording_id"], self.owned.recording);
        assert!(response.2["sources"].as_array().unwrap().is_empty());
        assert!(response.2.get("plugin_connection").is_none());
    }

    pub async fn check_replay(&self, request: &Value, resolution: &Value) {
        let replay = self.resolve(request).await;
        self.check_resolution(&replay, request);
        assert_eq!(&replay.2, resolution);
        let (status, error) = self
            .peer
            .request(&self.app, "POST", &self.path(), request.clone())
            .await;
        assert_eq!(status, StatusCode::NOT_FOUND);
        assert!(error.get("plugin_connection").is_none());
    }

    pub async fn refresh_peer_baseline_after_offering(&mut self, service_id: &str) {
        assert!(
            self.peer_before["services"]
                .as_array()
                .unwrap()
                .iter()
                .all(|service| service["id"] != service_id)
        );
        let current = Self::settings_for(&self.app, &self.peer).await;
        let mut previous = current.clone();
        let services = previous["services"].as_array_mut().unwrap();
        assert_eq!(
            services
                .iter()
                .filter(|service| service["id"] == service_id)
                .count(),
            1
        );
        services.retain(|service| service["id"] != service_id);
        assert_eq!(previous, self.peer_before);
        self.peer_before = current;
    }

    pub async fn check_preserved(
        &self,
        expected_settings: &Value,
        expected_choice: &Value,
        primary: &ProviderServer,
        previous: &ProviderServer,
        plugin: &FakePlugin,
        receipts: &Value,
    ) -> Value {
        let current = self.settings().await;
        let mut expected = expected_settings.clone();
        let mut actual = current.clone();
        for settings in [&mut expected, &mut actual] {
            for installation in settings["installations"].as_array_mut().unwrap() {
                installation.as_object_mut().unwrap().remove("health");
            }
        }
        assert_eq!(actual, expected);
        assert_eq!(
            Self::settings_for(&self.app, &self.peer).await,
            self.peer_before
        );
        assert_eq!(
            self.owner
                .get(
                    &self.app,
                    &format!("/api/v1/recordings/{}/source-choice", self.owned.recording)
                )
                .await,
            *expected_choice
        );
        assert_eq!(
            self.owner
                .get(
                    &self.app,
                    &format!(
                        "/api/v1/recordings/{}/lyrics/identities",
                        self.owned.recording
                    )
                )
                .await,
            self.identities
        );
        self.owned.assert_preserved().await;
        for path in [
            "v1/search",
            "v1/recordings/detail",
            "v1/recordings/metadata",
            "v1/sources",
            "v1/audio",
        ] {
            assert!(previous.requests(path).await.is_empty());
        }
        let serialized = serde_json::to_string(receipts).unwrap();
        for private in [
            OWNER_TOKEN,
            PEER_TOKEN,
            NEW_TOKEN,
            "Private title override",
            "primary/owned-recording",
            primary.url.as_str(),
            previous.url.as_str(),
            plugin.url.as_str(),
        ] {
            assert!(!serialized.contains(private));
        }
        assert_eq!(plugin.unexpected(), 0);
        current
    }
}

pub fn installation_by_id<'a>(settings: &'a Value, id: &Value) -> &'a Value {
    settings["installations"]
        .as_array()
        .unwrap()
        .iter()
        .find(|item| &item["id"] == id)
        .unwrap()
}
