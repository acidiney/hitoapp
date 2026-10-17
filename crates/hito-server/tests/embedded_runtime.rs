use hito_core::Credentials;
use hito_server::{Config, RuntimeConfig};
use std::time::Duration;
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpStream,
    sync::oneshot,
};

#[tokio::test]
async fn a_local_host_serves_shared_pages_and_joins_its_worker_without_owning_global_logging() {
    let directory = tempfile::tempdir().unwrap();
    let web = directory.path().join("web");
    std::fs::create_dir_all(web.join("assets")).unwrap();
    std::fs::write(web.join("assets/app.js"), "export {};\n").unwrap();
    let database = directory.path().join("hito.db");
    let config = RuntimeConfig::load(database.clone(), directory.path().join("music"), web, None)
        .await
        .unwrap();
    let host = config.core.host_device.clone();
    let services = hito_core::open(config.core).await.unwrap();
    for _ in 0..20 {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let config = Config {
            origin: format!("http://{address}"),
            ..config.http.clone()
        };
        let (stop, stopped) = oneshot::channel();
        let serving = tokio::spawn(hito_server::serve(
            listener,
            hito_server::router_with_config(services.clone(), config),
            services.clone(),
            async {
                let _ = stopped.await;
            },
        ));
        let mut stream = TcpStream::connect(address).await.unwrap();
        stream
            .write_all(
                format!("GET /library HTTP/1.1\r\nHost: {address}\r\nConnection: close\r\n\r\n")
                    .as_bytes(),
            )
            .await
            .unwrap();
        let mut response = String::new();
        tokio::time::timeout(Duration::from_secs(3), stream.read_to_string(&mut response))
            .await
            .unwrap()
            .unwrap();
        assert!(response.starts_with("HTTP/1.1 200"));
        assert!(response.contains("LoginPage"));
        assert!(response.contains("/assets/app.js"));
        stop.send(()).unwrap();
        tokio::time::timeout(Duration::from_secs(3), serving)
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        assert!(TcpStream::connect(address).await.is_err());
    }
    let reopened = RuntimeConfig::load(
        database,
        directory.path().join("music"),
        config.http.web_dir,
        None,
    )
    .await
    .unwrap();
    assert_eq!(reopened.core.host_device, host);
    assert!(
        hito_core::open(reopened.core)
            .await
            .unwrap()
            .healthy()
            .await
    );
}

#[tokio::test]
async fn dropping_the_host_future_does_not_leave_an_index_worker_running() {
    let directory = tempfile::tempdir().unwrap();
    let music = directory.path().join("music");
    std::fs::create_dir(&music).unwrap();
    let services = hito_core::open(hito_core::CoreConfig::new(
        directory.path().join("hito.db"),
        music,
        hito_core::HostDeviceId::new("desktop-test").unwrap(),
    ))
    .await
    .unwrap();
    let session = services
        .identity
        .setup(Credentials {
            username: "owner".into(),
            password: "desktop lifecycle fixture password".into(),
        })
        .await
        .unwrap();
    let actor = services
        .identity
        .authenticate(&session.token)
        .await
        .unwrap();
    let library = services
        .catalog
        .libraries(&actor)
        .await
        .unwrap()
        .remove(0)
        .id;
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let serving = tokio::spawn(hito_server::serve(
        listener,
        hito_server::router(services.clone()),
        services.clone(),
        std::future::pending(),
    ));
    let mut stream = TcpStream::connect(address).await.unwrap();
    stream
        .write_all(
            format!("GET /health HTTP/1.1\r\nHost: {address}\r\nConnection: close\r\n\r\n")
                .as_bytes(),
        )
        .await
        .unwrap();
    let mut response = String::new();
    tokio::time::timeout(Duration::from_secs(3), stream.read_to_string(&mut response))
        .await
        .unwrap()
        .unwrap();
    serving.abort();
    assert!(serving.await.unwrap_err().is_cancelled());
    let job = services
        .storage
        .enqueue_scan(&actor, &library)
        .await
        .unwrap()
        .remove(0);
    tokio::time::sleep(Duration::from_millis(1500)).await;
    assert_eq!(
        services.storage.job(&actor, &job.id).await.unwrap().state,
        "queued"
    );
}
