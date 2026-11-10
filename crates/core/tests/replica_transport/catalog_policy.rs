use super::{
    Endpoint, Remote, TOKEN,
    catalog::{Call, SNAPSHOT, metadata},
    fixture::{Reply, Server},
    json,
    raw::RawServer,
    transport::RemoteError,
};
use std::time::Duration;
use uuid::Uuid;

#[tokio::test]
async fn catalog_json_rejects_malformed_unknown_fields_and_ambiguous_headers() {
    for call in [Call::Begin, Call::Page] {
        for case in 0..6 {
            let mut reply = Reply::json(if matches!(call, Call::Begin) {
                metadata()
            } else {
                json!({"snapshot_id":SNAPSHOT,"ordinal":0,"recordings":[],"next_ordinal":null})
            });
            match case {
                0 => {
                    reply.chunks = vec![(Duration::ZERO, b"{private-malformed-json".to_vec())];
                    false
                }
                1 => reply
                    .headers
                    .append("content-type", "application/json".parse().unwrap()),
                2 => {
                    reply
                        .headers
                        .insert("content-encoding", "identity".parse().unwrap());
                    reply
                        .headers
                        .append("content-encoding", "identity".parse().unwrap())
                }
                3 => reply
                    .headers
                    .insert("content-encoding", "gzip".parse().unwrap())
                    .is_some(),
                4 => reply
                    .headers
                    .insert("content-type", "text/html".parse().unwrap())
                    .is_some(),
                _ => {
                    let mut value = if matches!(call, Call::Begin) {
                        metadata()
                    } else {
                        json!({"snapshot_id":SNAPSHOT,"ordinal":0,"recordings":[],"next_ordinal":null})
                    };
                    value["private_unknown_field"] = json!("private-title");
                    reply = Reply::json(value);
                    false
                }
            };
            let server = Server::scripted(vec![reply]).await;
            let remote = Remote::new(Endpoint::parse(&server.endpoint).unwrap()).unwrap();
            assert_eq!(
                call.run(&remote).await.unwrap_err(),
                RemoteError::InvalidReply,
                "{call:?} case {case}"
            );
            server.close().await;
        }
    }
}

#[tokio::test]
async fn catalog_json_limits_apply_to_declared_and_streamed_bytes_at_the_boundary() {
    for (call, limit, value) in [
        (Call::Begin, 65_536, metadata()),
        (
            Call::Page,
            262_144,
            json!({"snapshot_id":SNAPSHOT,"ordinal":0,"recordings":[],"next_ordinal":null}),
        ),
    ] {
        for declared in [false, true] {
            for extra in [0, 1] {
                let mut reply = Reply::json(value.clone());
                let mut bytes = serde_json::to_vec(&value).unwrap();
                bytes.resize(limit + extra, b' ');
                reply.chunks = vec![
                    (Duration::ZERO, bytes[..limit].to_vec()),
                    (Duration::ZERO, bytes[limit..].to_vec()),
                ];
                if declared {
                    reply.headers.insert(
                        "content-length",
                        (limit + extra).to_string().parse().unwrap(),
                    );
                }
                let server = Server::scripted(vec![reply]).await;
                let remote = Remote::new(Endpoint::parse(&server.endpoint).unwrap()).unwrap();
                if extra == 0 {
                    if matches!(call, Call::Page) {
                        let (_, count) = remote
                            .catalog_snapshot_page(
                                TOKEN,
                                "owned-account",
                                Uuid::parse_str(SNAPSHOT).unwrap(),
                                0,
                            )
                            .await
                            .unwrap();
                        assert_eq!(count, limit);
                    } else {
                        call.run(&remote).await.unwrap();
                    }
                } else {
                    assert_eq!(
                        call.run(&remote).await.unwrap_err(),
                        RemoteError::InvalidReply
                    );
                }
                server.close().await;
            }
        }
    }
}

#[tokio::test]
async fn empty_release_rejects_wrong_status_entity_framing_and_ambiguous_headers() {
    let mut cases = vec![
        "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: 2\r\n\r\n{}".to_string(),
        "HTTP/1.1 204 No Content\r\ncontent-length: 1\r\n\r\nx".into(),
        "HTTP/1.1 204 No Content\r\ntransfer-encoding: chunked\r\n\r\n0\r\n\r\n".into(),
        "HTTP/1.1 204 No Content\r\ncontent-type: application/json\r\ncontent-type: application/json\r\n\r\n".into(),
        "HTTP/1.1 204 No Content\r\ncontent-encoding: gzip\r\n\r\n".into(),
        "HTTP/1.1 204 No Content\r\ncontent-encoding: identity\r\ncontent-encoding: identity\r\n\r\n".into(),
    ];
    cases.push(format!(
        "HTTP/1.1 204 No Content\r\nx-owned-large: {}\r\n\r\n",
        "x".repeat(16_384)
    ));
    for response in cases {
        let server = RawServer::start(response.into_bytes(), false).await;
        let remote = Remote::new(Endpoint::parse(&server.endpoint).unwrap()).unwrap();
        assert_eq!(
            Call::Release.run(&remote).await.unwrap_err(),
            RemoteError::InvalidReply
        );
        server.close().await;
    }
    let server = RawServer::start(
        b"HTTP/1.1 204 No Content\r\ncontent-length: 0\r\n\r\n".to_vec(),
        false,
    )
    .await;
    let remote = Remote::new(Endpoint::parse(&server.endpoint).unwrap()).unwrap();
    Call::Release.run(&remote).await.unwrap();
    server.close().await;
}

#[tokio::test]
async fn catalog_redirects_never_forward_credentials_to_either_origin() {
    for call in Call::ALL {
        for same_origin in [false, true] {
            let destination = Server::start(vec![]).await;
            let mut reply = Reply::status(307);
            reply.headers.insert(
                "location",
                if same_origin {
                    "/private-redirect".into()
                } else {
                    format!("{}/private-redirect", destination.endpoint)
                }
                .parse()
                .unwrap(),
            );
            let server = Server::scripted(vec![reply]).await;
            let remote = Remote::new(Endpoint::parse(&server.endpoint).unwrap()).unwrap();
            assert_eq!(
                call.run(&remote).await.unwrap_err(),
                RemoteError::InvalidReply
            );
            assert_eq!(server.requests().len(), 1);
            assert!(destination.requests().is_empty());
            server.close().await;
            destination.close().await;
        }
    }
}

#[tokio::test]
async fn catalog_requests_ignore_process_proxy_settings() {
    const CHILD: &str = "HITO_CATALOG_PROXY_CHILD";
    if std::env::var_os(CHILD).is_none() {
        let mut command = std::process::Command::new(std::env::current_exe().unwrap());
        command
            .args([
                "--exact",
                "catalog_policy::catalog_requests_ignore_process_proxy_settings",
                "--nocapture",
            ])
            .env(CHILD, "1")
            .env("NO_PROXY", "")
            .env("no_proxy", "");
        for name in [
            "ALL_PROXY",
            "all_proxy",
            "HTTP_PROXY",
            "http_proxy",
            "HTTPS_PROXY",
            "https_proxy",
        ] {
            command.env(name, "http://127.0.0.1:9");
        }
        let result = command.output().unwrap();
        assert!(
            result.status.success(),
            "proxy isolation child failed: {}",
            String::from_utf8_lossy(&result.stdout)
        );
        return;
    }
    let mut empty = Reply::status(204);
    empty.headers.clear();
    empty.chunks.clear();
    let server = Server::scripted(vec![
        Reply::json(metadata()),
        Reply::json(
            json!({"snapshot_id":SNAPSHOT,"ordinal":0,"recordings":[],"next_ordinal":null}),
        ),
        empty,
    ])
    .await;
    let remote = Remote::new(Endpoint::parse(&server.endpoint).unwrap()).unwrap();
    for call in Call::ALL {
        call.run(&remote).await.unwrap();
    }
    assert_eq!(server.requests().len(), 3);
    server.close().await;
}

#[tokio::test]
async fn catalog_rejects_unsafe_credentials_and_oversize_requests_before_sending() {
    let server = Server::start(vec![]).await;
    let remote = Remote::new(Endpoint::parse(&server.endpoint).unwrap()).unwrap();
    let id = Uuid::parse_str(SNAPSHOT).unwrap();
    for (token, account) in [
        ("private\r\nunsafe", "owned-account"),
        (TOKEN, "private\r\nunsafe"),
        ("", "owned-account"),
        (TOKEN, ""),
    ] {
        assert!(matches!(
            remote
                .begin_catalog_snapshot(
                    token,
                    account,
                    &super::BeginCatalogSnapshot {
                        library_id: "private-library".into()
                    }
                )
                .await,
            Err(RemoteError::InvalidReply)
        ));
        assert!(matches!(
            remote.catalog_snapshot_page(token, account, id, 0).await,
            Err(RemoteError::InvalidReply)
        ));
        assert_eq!(
            remote
                .release_catalog_snapshot(token, account, id)
                .await
                .unwrap_err(),
            RemoteError::InvalidReply
        );
    }
    assert!(matches!(
        remote
            .begin_catalog_snapshot(
                TOKEN,
                "owned-account",
                &super::BeginCatalogSnapshot {
                    library_id: "x".repeat(262_144)
                }
            )
            .await,
        Err(RemoteError::InvalidRequest)
    ));
    assert!(
        server.requests().is_empty(),
        "unsafe catalog request reached the network"
    );
    server.close().await;
}
