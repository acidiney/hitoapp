use super::{
    Endpoint, Remote, TOKEN,
    fixture::{Reply, Server},
    json,
    transport::RemoteError,
};
use std::time::Duration;

#[tokio::test]
async fn redirects_never_forward_credentials_or_retry_even_with_the_same_origin() {
    for same_origin in [false, true] {
        let destination = Server::start(vec![]).await;
        let mut reply = Reply::status(307);
        reply.headers.insert(
            "location",
            if same_origin {
                "/private-redirect".to_owned()
            } else {
                format!("{}/private-redirect", destination.endpoint)
            }
            .parse()
            .unwrap(),
        );
        let server = Server::scripted(vec![reply]).await;
        let remote = Remote::new(Endpoint::parse(&server.endpoint).unwrap()).unwrap();
        assert_eq!(
            remote.me(TOKEN, "owned-account").await.unwrap_err(),
            RemoteError::InvalidReply
        );
        assert_eq!(server.requests().len(), 1);
        assert!(
            destination.requests().is_empty(),
            "credentials were forwarded to a redirect"
        );
        server.close().await;
        destination.close().await;
    }
}

#[tokio::test]
async fn malformed_ambiguous_and_non_json_replies_are_rejected() {
    let account = json!({"id":"owned-account","username":"listener","admin":false,"display_name":null,"avatar_version":null});
    let mut cases = Vec::new();
    let mut html = Reply::json(account.clone());
    html.headers
        .insert("content-type", "text/html".parse().unwrap());
    html.chunks = vec![(Duration::ZERO, b"<html>private-error-page</html>".to_vec())];
    cases.push(html);
    let mut malformed = Reply::json(account.clone());
    malformed.chunks = vec![(Duration::ZERO, b"{\"id\":\"owned-account\"".to_vec())];
    cases.push(malformed);
    let mut encoding = Reply::json(account.clone());
    encoding
        .headers
        .insert("content-encoding", "gzip".parse().unwrap());
    cases.push(encoding);
    let mut duplicated = Reply::json(account);
    duplicated
        .headers
        .append("content-type", "text/html".parse().unwrap());
    cases.push(duplicated);
    for reply in cases {
        let server = Server::scripted(vec![reply]).await;
        let remote = Remote::new(Endpoint::parse(&server.endpoint).unwrap()).unwrap();
        assert_eq!(
            remote.me(TOKEN, "owned-account").await.unwrap_err(),
            RemoteError::InvalidReply
        );
        server.close().await;
    }
    let mut ambiguous = json!({"sequence":1,"device_id":"replica-client","operation":super::fixture::playlist_batch("private-title").operations[0]});
    let publication = json!({
        "kind":"playlist_imported",
        "publication_id":"33333333-3333-4333-8333-333333333333",
        "snapshot":{
            "playlist_id":"22222222-2222-4222-8222-222222222222",
            "name":"private-import",
            "origin":{
                "installation_id":"installation",
                "installation_revision":1,
                "service_id":"service",
                "service_revision":1,
                "provider":"provider",
                "provider_playlist_id":"public-list",
                "snapshot":"revision",
                "attribution":{"label":"provider"},
                "imported_at":1
            },
            "recordings":[],
            "entries":[]
        }
    });
    assert!(serde_json::from_value::<hito_core::SyncEvent>(ambiguous.clone()).is_ok());
    assert!(
        serde_json::from_value::<hito_core::SyncEvent>(
            json!({"sequence":1,"publication":publication})
        )
        .is_ok()
    );
    ambiguous["publication"] = publication;
    let server = Server::start(vec![
        json!({"operations":[ambiguous],"cursor":1,"has_more":false}),
    ])
    .await;
    let remote = Remote::new(Endpoint::parse(&server.endpoint).unwrap()).unwrap();
    assert!(matches!(
        remote.pull(TOKEN, "owned-account", 0).await,
        Err(RemoteError::InvalidReply)
    ));
    server.close().await;
}

#[tokio::test]
async fn body_limits_apply_to_content_length_and_streamed_chunks() {
    for declared in [false, true] {
        let mut reply = Reply::json(json!({}));
        reply.chunks = vec![(Duration::ZERO, vec![b' '; 65_537])];
        if declared {
            reply
                .headers
                .insert("content-length", "65537".parse().unwrap());
        }
        let server = Server::scripted(vec![reply]).await;
        let remote = Remote::new(Endpoint::parse(&server.endpoint).unwrap()).unwrap();
        assert_eq!(
            remote.me(TOKEN, "owned-account").await.unwrap_err(),
            RemoteError::InvalidReply
        );
        server.close().await;
    }
    let mut large_page = Reply::json(json!({}));
    large_page.chunks = vec![
        (Duration::ZERO, vec![b' '; 16 * 1024 * 1024]),
        (Duration::ZERO, vec![b' '; 1]),
    ];
    let server = Server::scripted(vec![large_page]).await;
    let remote = Remote::new(Endpoint::parse(&server.endpoint).unwrap()).unwrap();
    assert!(matches!(
        remote.pull(TOKEN, "owned-account", 0).await,
        Err(RemoteError::InvalidReply)
    ));
    server.close().await;
}

#[tokio::test]
async fn sync_pages_can_exceed_the_small_identity_response_limit() {
    let mut reply = Reply::json(json!({"operations":[],"cursor":7,"has_more":false}));
    reply.chunks.push((Duration::ZERO, vec![b' '; 65_536]));
    let server = Server::scripted(vec![reply]).await;
    let remote = Remote::new(Endpoint::parse(&server.endpoint).unwrap()).unwrap();
    let page = remote.pull(TOKEN, "owned-account", 7).await.unwrap();
    assert_eq!(page.cursor, 7);
    server.close().await;
}
