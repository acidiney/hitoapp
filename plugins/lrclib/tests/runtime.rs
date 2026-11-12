use serde_json::{Value, json};
use std::{
    fs::OpenOptions,
    net::TcpListener,
    process::{Child, Command, Stdio},
    time::Duration,
};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

const TOKEN: &str = "owned-private-runtime-lrclib-token";

struct Runtime(Child);
impl Drop for Runtime {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

#[tokio::test]
async fn standalone_runtime_authenticates_and_cancels_held_lyrics_before_joined_shutdown() {
    for contact in [None, Some("  "), Some(" contact@example.invalid ")] {
        exercise_runtime_contact(contact).await;
    }
}

async fn exercise_runtime_contact(contact: Option<&str>) {
    let _ = rustls::crypto::ring::default_provider().install_default();
    let directory = tempfile::tempdir().unwrap();
    let credential = directory.path().join("private-service-token");
    std::fs::write(&credential, TOKEN).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&credential, std::fs::Permissions::from_mode(0o600)).unwrap();
    }
    let log_path = directory.path().join("runtime.jsonl");
    let log = OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(&log_path)
        .unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let endpoint = format!("http://{}/", listener.local_addr().unwrap());
    let (started_tx, started_rx) = tokio::sync::oneshot::channel();
    let expected_agent = match contact.map(str::trim).filter(|value| !value.is_empty()) {
        Some(_) => concat!(
            "HitoLRCLIB/",
            env!("CARGO_PKG_VERSION"),
            " (contact@example.invalid)"
        ),
        None => concat!("HitoLRCLIB/", env!("CARGO_PKG_VERSION")),
    };
    let provider = tokio::spawn(async move {
        let (mut connection, _) = listener.accept().await.unwrap();
        let mut request = Vec::new();
        let mut byte = [0u8; 1];
        while !request.ends_with(b"\r\n\r\n") {
            assert!(request.len() < 4096);
            connection.read_exact(&mut byte).await.unwrap();
            request.push(byte[0]);
        }
        assert!(request.starts_with(b"GET /api/get/41 HTTP/1.1\r\n"));
        let headers = String::from_utf8(request).unwrap();
        assert!(!headers.to_ascii_lowercase().contains("authorization:"));
        let agent = headers
            .lines()
            .filter_map(|line| line.split_once(':'))
            .find(|(name, _)| name.eq_ignore_ascii_case("user-agent"))
            .map(|(_, value)| value.trim());
        assert!(
            agent == Some(expected_agent),
            "runtime contact configuration controls its upstream agent"
        );
        connection.write_all(b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: 10000\r\nConnection: close\r\n\r\n{\"id\":41").await.unwrap();
        started_tx.send(()).unwrap();
        match connection.read(&mut byte).await {
            Ok(0) => {}
            Err(error) if error.kind() == std::io::ErrorKind::ConnectionReset => {}
            other => panic!("runtime left the owned upstream body open: {other:?}"),
        }
    });
    let reservation = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = reservation.local_addr().unwrap();
    drop(reservation);
    let mut runtime = Runtime(
        Command::new(env!("CARGO_BIN_EXE_hito-plugin-lrclib"))
            .args(["--listen", &address.to_string(), "--service-token-file"])
            .arg(&credential)
            .arg("--endpoint")
            .arg(&endpoint)
            .env_remove("HITO_METADATA_CONTACT")
            .envs(contact.map(|value| ("HITO_METADATA_CONTACT", value)))
            .env("RUST_LOG", "trace")
            .stdin(Stdio::null())
            .stdout(Stdio::from(log.try_clone().unwrap()))
            .stderr(Stdio::from(log))
            .spawn()
            .unwrap(),
    );
    let client = reqwest::Client::new();
    let base = format!("http://{address}/");
    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    loop {
        if let Ok(response) = client.get(format!("{base}v1/manifest")).send().await {
            assert_eq!(response.status(), 200);
            break;
        }
        assert!(
            runtime.0.try_wait().unwrap().is_none(),
            "runtime exited before actual HTTP readiness"
        );
        assert!(
            tokio::time::Instant::now() < deadline,
            "runtime never reached actual HTTP readiness"
        );
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    assert_eq!(
        client
            .get(format!("{base}v2/lyrics/capabilities"))
            .send()
            .await
            .unwrap()
            .status(),
        401
    );
    let pending = client
        .post(format!("{base}v2/lyrics"))
        .bearer_auth(TOKEN)
        .header("x-hito-plugin-protocol", "1")
        .json(&json!({"recording":{"id":"41","revision":null}}))
        .send();
    tokio::pin!(pending);
    tokio::select! {
        _ = &mut pending => panic!("runtime held response completed early"),
        result = started_rx => result.unwrap(),
    }
    #[cfg(unix)]
    {
        assert_eq!(
            unsafe { libc::kill(runtime.0.id() as i32, libc::SIGTERM) },
            0
        );
    }
    #[cfg(not(unix))]
    {
        runtime.0.kill().unwrap();
    }
    assert_eq!(
        tokio::time::timeout(Duration::from_secs(2), &mut pending)
            .await
            .unwrap()
            .unwrap()
            .status(),
        503
    );
    tokio::time::timeout(Duration::from_secs(2), provider)
        .await
        .unwrap()
        .unwrap();
    let deadline = tokio::time::Instant::now() + Duration::from_secs(3);
    loop {
        if let Some(status) = runtime.0.try_wait().unwrap() {
            assert!(status.success());
            break;
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "runtime failed to join shutdown"
        );
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    let logs = std::fs::read_to_string(&log_path).unwrap();
    for event in [
        "lrclib_service_ready",
        "lrclib_shutdown_requested",
        "lrclib_service_stopped",
    ] {
        assert!(logs.contains(event));
    }
    let events: Vec<Value> = logs
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    assert!(events.iter().any(|event| event["fields"]["event"]
        == "lrclib_plugin_request_finished"
        && event["fields"]["outcome"] == "cancelled"));
    for private in [
        TOKEN,
        credential.to_str().unwrap(),
        &endpoint,
        "Owned river",
        "Synthetic river",
        "contact@example.invalid",
    ] {
        assert!(!logs.contains(private), "private runtime data was logged");
    }
}

#[cfg(unix)]
#[test]
fn unsafe_operator_credentials_and_arguments_are_refused_without_private_output() {
    use std::ffi::OsString;
    use std::os::unix::fs::{PermissionsExt, symlink};
    let directory = tempfile::tempdir().unwrap();
    let write = |name: &str, value: &str, mode| {
        let path = directory.path().join(name);
        std::fs::write(&path, value).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(mode)).unwrap();
        path
    };
    let valid = write("private-valid-token", TOKEN, 0o600);
    let public = write("private-public-token", TOKEN, 0o644);
    let empty = write("private-empty-token", "", 0o600);
    let long = write("private-long-token", &"x".repeat(4099), 0o600);
    let newline = write(
        "private-newline-token",
        "owned\nprivate-runtime-token",
        0o600,
    );
    let linked = write("private-linked-token", TOKEN, 0o600);
    std::fs::hard_link(&linked, directory.path().join("private-other-link")).unwrap();
    let symbolic = directory.path().join("private-symbolic-token");
    symlink(&valid, &symbolic).unwrap();
    let mut cases: Vec<Vec<OsString>> = vec![
        vec![],
        vec![
            "--service-token-file".into(),
            "relative-private-token".into(),
        ],
    ];
    for path in [&public, &empty, &long, &newline, &linked, &symbolic] {
        cases.push(vec![
            "--service-token-file".into(),
            path.as_os_str().to_owned(),
        ]);
    }
    cases.push(vec![
        "--service-token-file".into(),
        valid.as_os_str().to_owned(),
        "--endpoint".into(),
        "https://owned-private-user:owned-private-password@lrclib.net/".into(),
    ]);
    cases.push(vec![
        "--service-token-file".into(),
        valid.as_os_str().to_owned(),
        "--listen".into(),
        "127.0.0.1:0".into(),
        "--listen".into(),
        "127.0.0.1:0".into(),
    ]);
    for (index, arguments) in cases.into_iter().enumerate() {
        let output = directory.path().join(format!("refused-{index}.jsonl"));
        let log = OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&output)
            .unwrap();
        let mut child = Runtime(
            Command::new(env!("CARGO_BIN_EXE_hito-plugin-lrclib"))
                .args(arguments)
                .env_remove("HITO_METADATA_CONTACT")
                .env("RUST_LOG", "trace")
                .stdin(Stdio::null())
                .stdout(Stdio::from(log.try_clone().unwrap()))
                .stderr(Stdio::from(log))
                .spawn()
                .unwrap(),
        );
        let deadline = std::time::Instant::now() + Duration::from_secs(3);
        loop {
            if let Some(status) = child.0.try_wait().unwrap() {
                assert!(!status.success());
                break;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "runtime accepted unsafe operator configuration"
            );
            std::thread::sleep(Duration::from_millis(10));
        }
        let logs = std::fs::read_to_string(output).unwrap();
        assert!(logs.contains("lrclib_service_failed"));
        for private in [
            TOKEN,
            directory.path().to_str().unwrap(),
            "relative-private-token",
            "owned-private-password",
            "owned\nprivate-runtime-token",
        ] {
            assert!(!logs.contains(private));
        }
    }
}

#[cfg(unix)]
#[test]
fn invalid_runtime_contacts_are_refused_without_values_or_readiness() {
    use std::{
        ffi::OsString,
        os::unix::{ffi::OsStringExt, fs::PermissionsExt},
    };
    let directory = tempfile::tempdir().unwrap();
    let credential = directory.path().join("private-contact-service-token");
    std::fs::write(&credential, TOKEN).unwrap();
    std::fs::set_permissions(&credential, std::fs::Permissions::from_mode(0o600)).unwrap();
    for (index, contact) in [
        OsString::from("owned-invalid contact"),
        OsString::from("owned-contact\nmarker"),
        OsString::from("x".repeat(129)),
        OsString::from_vec(vec![0xff]),
    ]
    .into_iter()
    .enumerate()
    {
        let log_path = directory
            .path()
            .join(format!("invalid-contact-{index}.jsonl"));
        let log = OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&log_path)
            .unwrap();
        let mut child = Runtime(
            Command::new(env!("CARGO_BIN_EXE_hito-plugin-lrclib"))
                .arg("--service-token-file")
                .arg(&credential)
                .args([
                    "--listen",
                    "127.0.0.1:0",
                    "--endpoint",
                    "http://127.0.0.1:9/",
                ])
                .env("HITO_METADATA_CONTACT", &contact)
                .env("RUST_LOG", "trace")
                .stdin(Stdio::null())
                .stdout(Stdio::from(log.try_clone().unwrap()))
                .stderr(Stdio::from(log))
                .spawn()
                .unwrap(),
        );
        let deadline = std::time::Instant::now() + Duration::from_secs(3);
        loop {
            if let Some(status) = child.0.try_wait().unwrap() {
                assert!(
                    !status.success(),
                    "invalid contact configuration fails before service admission"
                );
                break;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "invalid contact configuration was admitted"
            );
            std::thread::sleep(Duration::from_millis(10));
        }
        let logs = std::fs::read_to_string(log_path).unwrap();
        assert!(logs.contains("invalid_metadata_contact"));
        assert!(!logs.contains("lrclib_service_ready"));
        for value in [
            TOKEN,
            credential.to_str().unwrap(),
            "owned-invalid contact",
            "owned-contact\nmarker",
            &"x".repeat(129),
        ] {
            assert!(
                !logs.contains(value),
                "invalid contact diagnostics contain only safe reason codes"
            );
        }
    }
}
