use super::transport::RemoteError;
use super::{
    Credentials, Endpoint, Remote, TOKEN,
    fixture::{Reply, Server},
    json,
};

#[test]
fn endpoints_are_explicit_canonical_origins_and_allow_self_hosted_lan() {
    for (input, expected) in [
        ("HTTP://LOCALHOST:80", "http://localhost/"),
        ("https://EXAMPLE.test:443/", "https://example.test/"),
        ("http://192.168.1.5:8090/", "http://192.168.1.5:8090/"),
        ("http://[::1]:8090/", "http://[::1]:8090/"),
    ] {
        assert_eq!(Endpoint::parse(input).unwrap().as_str(), expected);
    }
    for input in [
        "ftp://example.test/",
        "https://private:secret@example.test/",
        "https://@example.test/",
        "https://example.test/sub",
        "https://example.test/?private",
        "https://example.test/#private",
        " https://example.test/",
        "https://exam\nple.test/",
        "example.test",
    ] {
        assert!(
            Endpoint::parse(input).is_err(),
            "unsafe or ambiguous endpoint was accepted"
        );
    }
}

#[tokio::test]
async fn rejects_invalid_server_tokens_and_does_not_send_unsafe_headers() {
    for token in [
        String::new(),
        "private\r\nAuthorization: unsafe".into(),
        "two words".into(),
        "é".into(),
        "a".repeat(4097),
    ] {
        let server = Server::start(vec![
            json!({"token":token,"user":super::fixture::account("owned-account","listener")}),
        ])
        .await;
        let remote = Remote::new(Endpoint::parse(&server.endpoint).unwrap()).unwrap();
        assert!(matches!(
            remote
                .login(Credentials {
                    username: "listener".into(),
                    password: "private-password".into()
                })
                .await,
            Err(RemoteError::InvalidReply)
        ));
        assert!(matches!(
            remote.me(&token, "owned-account").await,
            Err(RemoteError::InvalidReply)
        ));
        for account in ["", "another\r\naccount", "two accounts"] {
            assert!(matches!(
                remote.me(TOKEN, account).await,
                Err(RemoteError::InvalidReply)
            ));
        }
        assert_eq!(
            server.requests().len(),
            1,
            "invalid headers reached the network"
        );
        server.close().await;
    }
}

#[tokio::test]
async fn me_requires_the_requested_account_identity() {
    let server=Server::start(vec![json!({"id":"another-account","username":"listener","admin":true,"display_name":null,"avatar_version":null})]).await;
    let remote = Remote::new(Endpoint::parse(&server.endpoint).unwrap()).unwrap();
    assert!(matches!(
        remote.me(TOKEN, "owned-account").await,
        Err(RemoteError::InvalidReply)
    ));
    server.close().await;
}

#[tokio::test]
async fn classifies_http_errors_without_consuming_or_exposing_error_bodies() {
    let cases = [
        (401, RemoteError::Unauthorized),
        (423, RemoteError::Locked),
        (403, RemoteError::Forbidden),
        (409, RemoteError::Conflict),
        (
            429,
            RemoteError::RateLimited {
                retry_after: Some(std::time::Duration::from_secs(31)),
            },
        ),
        (500, RemoteError::Unavailable),
        (503, RemoteError::Unavailable),
        (404, RemoteError::NotFound),
    ];
    for (status, expected) in cases {
        let mut reply = Reply::status(status);
        reply.headers.insert("retry-after", "31".parse().unwrap());
        let server = Server::scripted(vec![reply]).await;
        let remote = Remote::new(Endpoint::parse(&server.endpoint).unwrap()).unwrap();
        assert_eq!(
            remote.me(TOKEN, "owned-account").await.unwrap_err(),
            expected
        );
        assert_eq!(
            server.requests().len(),
            1,
            "transport retried an error implicitly"
        );
        server.close().await;
    }
}

#[tokio::test]
async fn outgoing_json_cannot_exceed_the_actual_hito_http_body_limit() {
    let server = Server::start(vec![
        json!({"token":TOKEN,"user":super::fixture::account("owned-account","listener")}),
    ])
    .await;
    let remote = Remote::new(Endpoint::parse(&server.endpoint).unwrap()).unwrap();
    assert!(matches!(
        remote
            .login(Credentials {
                username: "listener".into(),
                password: "p".repeat(262_144)
            })
            .await,
        Err(RemoteError::InvalidRequest)
    ));
    assert!(
        server.requests().is_empty(),
        "oversized credentials reached the server"
    );
    let batch = super::fixture::playlist_batch(&"n".repeat(262_144));
    assert!(matches!(
        remote.push(TOKEN, "owned-account", &batch).await,
        Err(RemoteError::InvalidRequest)
    ));
    assert!(
        server.requests().is_empty(),
        "oversized sync batch reached the server"
    );
    server.close().await;
}
