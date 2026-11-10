pub use hito_core::{
    AuthSession, BeginCatalogSnapshot, CatalogSnapshot, CatalogSnapshotPage, Credentials, Error,
    Result, SyncBatch, SyncPage, SyncReceipt, User,
};
mod catalog;
mod catalog_lifetimes;
mod catalog_policy;
mod fixture;
mod lifetimes;
mod privacy;
mod raw;
mod replies;
#[path = "../../src/replicas/transport/mod.rs"]
mod transport;
mod validation;

use fixture::Server;
use serde_json::json;
use transport::{Endpoint, Remote};

const TOKEN: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

#[tokio::test]
async fn exchanges_actual_hito_requests_and_replies_with_the_account_fence() {
    let account = json!({"id":"owned-account","username":"private-listener","admin":false,"display_name":null,"avatar_version":null});
    let server = Server::start(vec![
        json!({"token":TOKEN,"user":account}),
        account.clone(),
        json!({"acknowledged":["11111111-1111-4111-8111-111111111111"]}),
        json!({"operations":[],"cursor":7,"has_more":false}),
    ])
    .await;
    let remote = Remote::new(Endpoint::parse(&server.endpoint).unwrap()).unwrap();
    let session = remote
        .login(Credentials {
            username: "private-listener".into(),
            password: "private-owned-password".into(),
        })
        .await
        .unwrap();
    assert_eq!(session.token, TOKEN);
    let user = remote.me(&session.token, &session.user.id).await.unwrap();
    assert_eq!(user.id, "owned-account");
    let batch = fixture::playlist_batch("Private offline list");
    let receipt = remote.push(&session.token, &user.id, &batch).await.unwrap();
    assert_eq!(receipt.acknowledged[0], batch.operations[0].id);
    let page = remote.pull(&session.token, &user.id, 7).await.unwrap();
    assert_eq!(page.cursor, 7);
    let requests = server.requests();
    assert_eq!(
        requests
            .iter()
            .map(|r| (r.method.as_str(), r.path.as_str()))
            .collect::<Vec<_>>(),
        vec![
            ("POST", "/api/v1/login"),
            ("GET", "/api/v1/me"),
            ("POST", "/api/v1/sync"),
            ("GET", "/api/v1/sync?cursor=7")
        ]
    );
    assert_eq!(
        requests[0].body,
        json!({"username":"private-listener","password":"private-owned-password"})
    );
    assert!(!requests[0].headers.contains_key("authorization"));
    for request in &requests {
        assert!(!request.headers.contains_key("origin"));
        assert_eq!(
            request.headers.get("x-hito-request").map(String::as_str),
            Some("1")
        );
    }
    for request in &requests[1..] {
        assert_eq!(
            request.headers.get("authorization"),
            Some(&format!("Bearer {TOKEN}"))
        );
        assert_eq!(
            request.headers.get("x-hito-account").map(String::as_str),
            Some("owned-account")
        );
        assert!(!request.headers.contains_key("cookie"));
    }
    assert_eq!(requests[2].body, serde_json::to_value(batch).unwrap());
    server.close().await;
}

#[tokio::test]
async fn server_issued_bearer_values_are_opaque_and_not_bound_to_the_current_generator() {
    for token in ["owned-short-session".to_string(), "b".repeat(512)] {
        let account = json!({"id":"owned-account","username":"listener","admin":false,"display_name":null,"avatar_version":null});
        let server = Server::start(vec![json!({"token":token,"user":account}), account]).await;
        let remote = Remote::new(Endpoint::parse(&server.endpoint).unwrap()).unwrap();
        let session = remote
            .login(Credentials {
                username: "listener".into(),
                password: "owned-password".into(),
            })
            .await
            .unwrap();
        assert_eq!(session.token, token);
        assert_eq!(
            remote
                .me(&session.token, &session.user.id)
                .await
                .unwrap()
                .id,
            "owned-account"
        );
        assert_eq!(
            server.requests()[1].headers.get("authorization"),
            Some(&format!("Bearer {token}"))
        );
        server.close().await;
    }
}

#[tokio::test]
async fn accepts_case_insensitive_json_media_type_from_a_real_http_response() {
    let mut reply = fixture::Reply::json(
        json!({"id":"owned-account","username":"listener","admin":false,"display_name":null,"avatar_version":null}),
    );
    reply.headers.insert(
        "content-type",
        "Application/JSON; charset=utf-8".parse().unwrap(),
    );
    let server = Server::scripted(vec![reply]).await;
    let remote = Remote::new(Endpoint::parse(&server.endpoint).unwrap()).unwrap();
    assert_eq!(
        remote.me(TOKEN, "owned-account").await.unwrap().id,
        "owned-account"
    );
    server.close().await;
}
