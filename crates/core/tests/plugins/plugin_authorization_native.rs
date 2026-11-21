use axum::{Router, extract::State, routing::post};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use hito_core::test_support::content_service as fixture;
use hito_core::test_support::support;
use hito_core::{
    Actor, Credentials, Error, InstallPlugin, OfferPluginService, PluginAuthorization,
    PluginAuthorizationCallback, PluginAuthorizationCompletion, PluginAuthorizationOutcome,
    PluginAuthorizationRedirect, PluginAuthorizationReturn, PluginAuthorizationState,
    PluginConnectionState, Services, SetPin,
};
use serde_json::json;
use sha2::{Digest, Sha256};
use sqlx::{Connection, SqliteConnection, sqlite::SqliteConnectOptions};
use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex},
};
use zeroize::Zeroizing;

const CALLBACK: &str = "https://hito.test/api/v1/plugin-authorization/callback";
const CODE: &str = "owned-native-provider-code";
const VERIFIER: &str = "owned-native-verifier-0123456789-abcdefghijklmnopqrstuvwxyz";

type Requests = Arc<Mutex<Vec<BTreeMap<String, String>>>>;

struct Fixture {
    hito: Services,
    actor: Actor,
    token: String,
    installation: String,
    revision: i64,
    requests: Requests,
    database: std::path::PathBuf,
    _provider: fixture::ProviderServer,
    _directory: tempfile::TempDir,
}

struct Started {
    attempt: PluginAuthorization,
    state: String,
}

fn verifier_hash(verifier: &str) -> String {
    URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()))
}

async fn token_endpoint() -> (String, Requests) {
    let requests = Requests::default();
    let router = Router::new()
        .route(
            "/token",
            post(|State(requests): State<Requests>, body: String| async move {
                requests.lock().unwrap().push(
                    url::form_urlencoded::parse(body.as_bytes())
                        .map(|(key, value)| (key.into_owned(), value.into_owned()))
                        .collect(),
                );
                (
                    [("content-type", "application/json")],
                    json!({"access_token": "owned-native-access", "token_type": "Bearer", "expires_in": 3600, "refresh_token": "owned-native-refresh"}).to_string(),
                )
            }),
        )
        .with_state(requests.clone());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/token", listener.local_addr().unwrap());
    tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
    (url, requests)
}

async fn fixture() -> Fixture {
    let (token_url, requests) = token_endpoint().await;
    let provider = fixture::ProviderServer::start().await;
    provider
        .reply(
            "v1/manifest",
            json!({
                "id": "org.hito.native-fixture",
                "name": "Native fixture",
                "protocol_version": 1,
                "capabilities": ["playlists"],
                "authorization": {
                    "kind": "oauth2_pkce",
                    "client_id": "owned-native-client",
                    "authorization_endpoint": "https://accounts.example/authorize",
                    "token_endpoint": token_url,
                    "scopes": ["playlist-read-private"]
                }
            }),
        )
        .await;
    let (directory, hito, actor, _library, _music) = support::library("native-host").await;
    let token = hito
        .identity
        .login(Credentials {
            username: "admin".into(),
            password: "indexing test password".into(),
        })
        .await
        .unwrap()
        .token;
    let actor_session = hito.identity.authenticate(&token).await.unwrap();
    let service = hito
        .plugins
        .offer_service(
            &actor,
            OfferPluginService {
                name: "Native fixture".into(),
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
        database: directory.path().join("hito.db"),
        hito,
        actor: actor_session,
        token,
        installation: installation.id,
        revision: installation.revision,
        requests,
        _provider: provider,
        _directory: directory,
    }
}

impl Fixture {
    async fn begin_with(&self, redirect: &PluginAuthorizationRedirect) -> Started {
        let attempt = self
            .hito
            .plugins
            .begin_authorization(&self.actor, &self.installation, self.revision, redirect)
            .await
            .unwrap();
        let url: url::Url = attempt.authorization_url.parse().unwrap();
        let state = url
            .query_pairs()
            .find(|(key, _)| key == "state")
            .unwrap()
            .1
            .into_owned();
        Started { attempt, state }
    }

    async fn begin(&self) -> Started {
        self.begin_with(
            &PluginAuthorizationRedirect::native(CALLBACK, &verifier_hash(VERIFIER)).unwrap(),
        )
        .await
    }

    async fn arrive(&self, query: &str) -> Option<PluginAuthorizationReturn> {
        let callback = PluginAuthorizationCallback::parse(Some(query)).unwrap();
        self.hito
            .plugins
            .return_native_authorization(&callback)
            .await
            .unwrap()
    }

    async fn approve(&self, started: &Started) -> String {
        let Some(PluginAuthorizationReturn::App(app)) = self
            .arrive(&format!("code={CODE}&state={}", started.state))
            .await
        else {
            panic!("A native attempt must return to the app");
        };
        let url: url::Url = app.as_str().parse().unwrap();
        let pairs: BTreeMap<String, String> = url
            .query_pairs()
            .map(|(key, value)| (key.into_owned(), value.into_owned()))
            .collect();
        pairs["return"].clone()
    }

    async fn complete_as(
        &self,
        actor: &Actor,
        started: &Started,
        verifier: &str,
        secret: &str,
    ) -> hito_core::Result<PluginAuthorizationOutcome> {
        self.hito
            .plugins
            .complete_native_authorization(
                actor,
                &self.installation,
                &started.attempt.id,
                completion(verifier, secret),
            )
            .await
    }

    async fn complete(
        &self,
        started: &Started,
        verifier: &str,
        secret: &str,
    ) -> hito_core::Result<PluginAuthorizationOutcome> {
        self.complete_as(&self.actor, started, verifier, secret)
            .await
    }

    async fn status(&self, started: &Started) -> PluginAuthorizationState {
        self.hito
            .plugins
            .authorization_status(&self.actor, &self.installation, &started.attempt.id)
            .await
            .unwrap()
            .status
    }

    async fn stored(&self, started: &Started) -> (String, Option<Vec<u8>>, Option<Vec<u8>>) {
        let mut connection =
            SqliteConnection::connect_with(&SqliteConnectOptions::new().filename(&self.database))
                .await
                .unwrap();
        let row = sqlx::query_as(
            "SELECT status,returned_code,return_hash FROM plugin_authorization_attempts WHERE id = ?",
        )
        .bind(&started.attempt.id)
        .fetch_optional(&mut connection)
        .await
        .unwrap()
        .unwrap_or_else(|| ("absent".into(), None, None));
        connection.close().await.unwrap();
        row
    }

    async fn sql(&self, statement: &str) {
        let mut connection =
            SqliteConnection::connect_with(&SqliteConnectOptions::new().filename(&self.database))
                .await
                .unwrap();
        sqlx::query(statement)
            .execute(&mut connection)
            .await
            .unwrap();
        connection.close().await.unwrap();
    }

    async fn connected(&self) -> bool {
        self.hito
            .plugins
            .connection_status(&self.actor, &self.installation)
            .await
            .unwrap()
            .status
            == PluginConnectionState::Connected
    }

    fn exchanges(&self) -> usize {
        self.requests.lock().unwrap().len()
    }
}

fn completion(verifier: &str, secret: &str) -> PluginAuthorizationCompletion {
    PluginAuthorizationCompletion {
        verifier: Zeroizing::new(verifier.into()),
        return_secret: Zeroizing::new(secret.into()),
    }
}

fn is_cleared(stored: &(String, Option<Vec<u8>>, Option<Vec<u8>>)) -> bool {
    stored.1.is_none() && stored.2.is_none()
}

#[test]
fn a_native_redirect_needs_the_public_callback_and_a_sha256_verifier_hash() {
    let hash = verifier_hash(VERIFIER);
    assert!(PluginAuthorizationRedirect::native(CALLBACK, &hash).is_ok());
    for refused in [
        "",
        "short",
        &hash[..42],
        &format!("{hash}A"),
        &format!("{}=", &hash[..42]),
        &hash.replace(|_: char| true, "+"),
        &URL_SAFE_NO_PAD.encode([7u8; 31]),
        &URL_SAFE_NO_PAD.encode([7u8; 33]),
    ] {
        assert!(
            PluginAuthorizationRedirect::native(CALLBACK, refused).is_err(),
            "{refused}"
        );
    }
    for callback in [
        "hito://plugin-authorization/return",
        "https://hito.test/other",
        "http://hito.test/api/v1/plugin-authorization/callback",
    ] {
        assert!(PluginAuthorizationRedirect::native(callback, &hash).is_err());
    }
}

#[tokio::test]
async fn a_native_sign_in_parks_the_code_returns_to_the_app_and_completes_with_both_secrets() {
    let fixture = fixture().await;
    let started = fixture.begin().await;
    let Some(PluginAuthorizationReturn::App(app)) = fixture
        .arrive(&format!("code={CODE}&state={}", started.state))
        .await
    else {
        panic!("A native attempt must return to the app");
    };
    let url: url::Url = app.as_str().parse().unwrap();
    assert_eq!(url.scheme(), "hito");
    assert_eq!(url.host_str(), Some("plugin-authorization"));
    assert_eq!(url.path(), "/return");
    let pairs: BTreeMap<String, String> = url
        .query_pairs()
        .map(|(key, value)| (key.into_owned(), value.into_owned()))
        .collect();
    assert_eq!(
        pairs.keys().map(String::as_str).collect::<Vec<_>>(),
        ["attempt", "return"]
    );
    assert_eq!(pairs["attempt"], started.attempt.id);
    assert!(pairs["return"].len() >= 43);
    for private in [
        CODE,
        started.state.as_str(),
        VERIFIER,
        &verifier_hash(VERIFIER),
    ] {
        assert!(!app.as_str().contains(private));
    }
    assert_eq!(fixture.exchanges(), 0);
    assert!(matches!(
        fixture.status(&started).await,
        PluginAuthorizationState::Returned
    ));
    let parked = fixture.stored(&started).await;
    assert_eq!(parked.0, "returned");
    let sealed = parked.1.clone().unwrap();
    assert!(
        !sealed
            .windows(CODE.len())
            .any(|window| window == CODE.as_bytes())
    );
    assert_eq!(
        parked.2.clone().unwrap(),
        Sha256::digest(pairs["return"].as_bytes()).to_vec()
    );

    let outcome = fixture
        .complete(&started, VERIFIER, &pairs["return"])
        .await
        .unwrap();
    assert_eq!(outcome, PluginAuthorizationOutcome::Connected);
    let requests = fixture.requests.lock().unwrap().clone();
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0]["code"], CODE);
    assert_eq!(requests[0]["redirect_uri"], CALLBACK);
    assert!(requests[0]["code_verifier"].len() >= 43);
    assert_ne!(requests[0]["code_verifier"], VERIFIER);
    assert!(matches!(
        fixture.status(&started).await,
        PluginAuthorizationState::Completed
    ));
    assert!(is_cleared(&fixture.stored(&started).await));
    assert!(fixture.connected().await);
}

#[tokio::test]
async fn a_reused_verifier_or_a_replayed_callback_exchanges_nothing_more() {
    let fixture = fixture().await;
    let started = fixture.begin().await;
    let secret = fixture.approve(&started).await;
    assert!(matches!(
        fixture
            .arrive(&format!("code=owned-replayed-code&state={}", started.state))
            .await,
        Some(PluginAuthorizationReturn::Closed(
            PluginAuthorizationOutcome::Used
        ))
    ));
    assert_eq!(
        fixture.complete(&started, VERIFIER, &secret).await.unwrap(),
        PluginAuthorizationOutcome::Connected
    );
    assert_eq!(fixture.requests.lock().unwrap()[0]["code"], CODE);
    assert_eq!(
        fixture.complete(&started, VERIFIER, &secret).await.unwrap(),
        PluginAuthorizationOutcome::Used
    );
    assert!(matches!(
        fixture
            .arrive(&format!("code={CODE}&state={}", started.state))
            .await,
        Some(PluginAuthorizationReturn::Closed(
            PluginAuthorizationOutcome::Used
        ))
    ));
    assert_eq!(fixture.exchanges(), 1);
}

#[tokio::test]
async fn a_callback_without_a_native_attempt_is_left_to_the_other_returns() {
    let fixture = fixture().await;
    let session = fixture
        .begin_with(&PluginAuthorizationRedirect::new(CALLBACK).unwrap())
        .await;
    let desktop = fixture
        .begin_with(
            &PluginAuthorizationRedirect::desktop_loopback(
                "http://127.0.0.1:47321/api/v1/plugin-authorization/callback",
            )
            .unwrap(),
        )
        .await;
    for state in [
        session.state.as_str(),
        desktop.state.as_str(),
        "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA",
    ] {
        assert!(
            fixture
                .arrive(&format!("code={CODE}&state={state}"))
                .await
                .is_none()
        );
    }
    assert!(matches!(
        fixture.status(&session).await,
        PluginAuthorizationState::Pending
    ));
    assert!(matches!(
        fixture.status(&desktop).await,
        PluginAuthorizationState::Pending
    ));
    assert_eq!(fixture.exchanges(), 0);
}

#[tokio::test]
async fn another_account_or_another_session_cannot_complete_the_attempt() {
    let fixture = fixture().await;
    let started = fixture.begin().await;
    let secret = fixture.approve(&started).await;
    let admin = fixture
        .hito
        .identity
        .authenticate(&fixture.token)
        .await
        .unwrap();
    fixture
        .hito
        .identity
        .create_user(
            &admin,
            Credentials {
                username: "member".into(),
                password: "member native test password".into(),
            },
        )
        .await
        .unwrap();
    let member = fixture
        .hito
        .identity
        .login(Credentials {
            username: "member".into(),
            password: "member native test password".into(),
        })
        .await
        .unwrap();
    let member = fixture
        .hito
        .identity
        .authenticate(&member.token)
        .await
        .unwrap();
    let other_session = fixture
        .hito
        .identity
        .login(Credentials {
            username: "admin".into(),
            password: "indexing test password".into(),
        })
        .await
        .unwrap();
    let other_session = fixture
        .hito
        .identity
        .authenticate(&other_session.token)
        .await
        .unwrap();
    for actor in [&member, &other_session] {
        assert!(matches!(
            fixture
                .complete_as(actor, &started, VERIFIER, &secret)
                .await,
            Err(Error::NotFound)
        ));
    }
    assert_eq!(fixture.exchanges(), 0);
    assert_eq!(fixture.stored(&started).await.0, "returned");
    assert_eq!(
        fixture.complete(&started, VERIFIER, &secret).await.unwrap(),
        PluginAuthorizationOutcome::Connected
    );
}

#[tokio::test]
async fn a_revoked_session_cannot_complete_and_takes_its_parked_code_with_it() {
    let fixture = fixture().await;
    let started = fixture.begin().await;
    let secret = fixture.approve(&started).await;
    fixture.hito.identity.logout(&fixture.token).await.unwrap();
    assert!(matches!(
        fixture.complete(&started, VERIFIER, &secret).await,
        Err(Error::Unauthorized)
    ));
    assert_eq!(fixture.stored(&started).await.0, "absent");
    assert_eq!(fixture.exchanges(), 0);
}

#[tokio::test]
async fn a_locked_session_cannot_complete_and_the_attempt_stays_unused() {
    let fixture = fixture().await;
    let started = fixture.begin().await;
    let secret = fixture.approve(&started).await;
    fixture
        .hito
        .identity
        .set_pin(
            &fixture.actor,
            SetPin {
                password: "indexing test password".into(),
                pin: Some("2468".into()),
                idle_minutes: 5,
            },
        )
        .await
        .unwrap();
    fixture.hito.identity.lock(&fixture.actor).await.unwrap();
    assert!(matches!(
        fixture.complete(&started, VERIFIER, &secret).await,
        Err(Error::Locked)
    ));
    assert_eq!(fixture.stored(&started).await.0, "returned");
    assert_eq!(fixture.exchanges(), 0);
}

#[tokio::test]
async fn a_wrong_verifier_or_return_secret_closes_the_attempt_without_an_exchange() {
    let fixture = fixture().await;
    let wrong_verifier = fixture.begin().await;
    let secret = fixture.approve(&wrong_verifier).await;
    assert_eq!(
        fixture
            .complete(
                &wrong_verifier,
                "owned-native-verifier-of-someone-else-0123456789",
                &secret
            )
            .await
            .unwrap(),
        PluginAuthorizationOutcome::Invalid
    );
    let missing_secret = fixture.begin().await;
    let real_secret = fixture.approve(&missing_secret).await;
    assert_eq!(
        fixture
            .complete(&missing_secret, VERIFIER, "")
            .await
            .unwrap(),
        PluginAuthorizationOutcome::Invalid
    );
    for (started, secret) in [(&wrong_verifier, &secret), (&missing_secret, &real_secret)] {
        let stored = fixture.stored(started).await;
        assert_eq!(stored.0, "failed");
        assert!(is_cleared(&stored));
        assert_eq!(
            fixture.complete(started, VERIFIER, secret).await.unwrap(),
            PluginAuthorizationOutcome::Used
        );
    }
    assert_eq!(fixture.exchanges(), 0);
    assert!(!fixture.connected().await);
}

#[tokio::test]
async fn completing_before_the_provider_returns_changes_nothing() {
    let fixture = fixture().await;
    let started = fixture.begin().await;
    assert_eq!(
        fixture
            .complete(
                &started,
                VERIFIER,
                "owned-guessed-return-secret-0123456789abcdef"
            )
            .await
            .unwrap(),
        PluginAuthorizationOutcome::Invalid
    );
    assert!(matches!(
        fixture.status(&started).await,
        PluginAuthorizationState::Pending
    ));
    let secret = fixture.approve(&started).await;
    assert_eq!(
        fixture.complete(&started, VERIFIER, &secret).await.unwrap(),
        PluginAuthorizationOutcome::Connected
    );
}

#[tokio::test]
async fn an_expired_attempt_is_refused_at_completion_and_its_parked_code_is_cleared() {
    let fixture = fixture().await;
    let started = fixture.begin().await;
    let secret = fixture.approve(&started).await;
    fixture
        .sql("UPDATE plugin_authorization_attempts SET created_at = unixepoch() - 601, expires_at = unixepoch() - 1")
        .await;
    assert_eq!(
        fixture.complete(&started, VERIFIER, &secret).await.unwrap(),
        PluginAuthorizationOutcome::Expired
    );
    let stored = fixture.stored(&started).await;
    assert_eq!(stored.0, "failed");
    assert!(is_cleared(&stored));
    assert_eq!(fixture.exchanges(), 0);
}

#[tokio::test]
async fn the_expiry_sweep_clears_every_expired_parked_code() {
    let fixture = fixture().await;
    let mut parked = Vec::new();
    for _ in 0..3 {
        let started = fixture.begin().await;
        fixture.approve(&started).await;
        parked.push(started);
    }
    fixture
        .sql("UPDATE plugin_authorization_attempts SET created_at = unixepoch() - 601, expires_at = unixepoch() - 1")
        .await;
    fixture
        .sql("UPDATE plugin_authorization_attempts SET created_at = unixepoch() - 300, expires_at = unixepoch() + 300 WHERE id = (SELECT id FROM plugin_authorization_attempts ORDER BY id LIMIT 1)")
        .await;
    fixture
        .sql("WITH RECURSIVE n(i) AS (SELECT 1 UNION ALL SELECT i + 1 FROM n WHERE i < 64)
            INSERT INTO plugin_authorization_attempts(id,user_id,session_hash,installation_id,service_id,installation_revision,service_revision,redirect_uri,state_hash,pending_secret,created_at,expires_at,status,delivery)
            SELECT 'older-' || i,a.user_id,a.session_hash,a.installation_id,a.service_id,a.installation_revision,a.service_revision,a.redirect_uri,randomblob(32),zeroblob(74),unixepoch() - 1000,unixepoch() - 900,'failed','session'
            FROM n CROSS JOIN (SELECT * FROM plugin_authorization_attempts LIMIT 1) a")
        .await;
    fixture.begin().await;
    let mut states = Vec::new();
    for started in &parked {
        let stored = fixture.stored(started).await;
        if stored.0 == "returned" {
            assert!(stored.1.is_some(), "A live attempt keeps its parked code");
        } else {
            assert!(
                is_cleared(&stored),
                "An expired attempt keeps no parked code"
            );
        }
        states.push(stored.0);
    }
    states.sort();
    assert_eq!(states, ["failed", "failed", "returned"]);
}

#[tokio::test]
async fn disconnecting_closes_a_returned_attempt_and_clears_its_parked_code() {
    let fixture = fixture().await;
    let started = fixture.begin().await;
    let secret = fixture.approve(&started).await;
    fixture
        .hito
        .plugins
        .disconnect(&fixture.actor, &fixture.installation)
        .await
        .unwrap();
    let stored = fixture.stored(&started).await;
    assert_eq!(stored.0, "failed");
    assert!(is_cleared(&stored));
    assert_eq!(
        fixture.complete(&started, VERIFIER, &secret).await.unwrap(),
        PluginAuthorizationOutcome::Used
    );
    assert_eq!(fixture.exchanges(), 0);
}

#[tokio::test]
async fn a_provider_denial_closes_a_native_attempt_without_a_return_secret() {
    let fixture = fixture().await;
    let started = fixture.begin().await;
    assert!(matches!(
        fixture
            .arrive(&format!("error=access_denied&state={}", started.state))
            .await,
        Some(PluginAuthorizationReturn::Closed(
            PluginAuthorizationOutcome::Denied
        ))
    ));
    let stored = fixture.stored(&started).await;
    assert_eq!(stored.0, "failed");
    assert!(is_cleared(&stored));
}
