#![cfg(unix)]
#[allow(dead_code)]
#[path = "support/binary.rs"]
mod binary;
use hito_plugin_sdk::{SearchPage, SourcePage};
use std::{
    fs::{self, File},
    io::{Read, Write},
    net::{SocketAddr, TcpListener, TcpStream},
    process::{Child, Command, Stdio},
    time::{Duration, Instant},
};
struct Process(Child);
impl Drop for Process {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

#[test]
fn sigterm_drains_an_active_audio_response_and_joins_the_binary() {
    let directory = tempfile::tempdir().unwrap();
    let music = directory.path().join("music");
    fs::create_dir(&music).unwrap();
    let file = music.join("private-shutdown-file.wav");
    let size = 32 * 1024 * 1024u32;
    let mut header = b"RIFF".to_vec();
    header.extend((size + 36).to_le_bytes());
    header.extend(b"WAVEfmt ");
    header.extend(16u32.to_le_bytes());
    header.extend(1u16.to_le_bytes());
    header.extend(1u16.to_le_bytes());
    header.extend(8000u32.to_le_bytes());
    header.extend(16000u32.to_le_bytes());
    header.extend(2u16.to_le_bytes());
    header.extend(16u16.to_le_bytes());
    header.extend(b"data");
    header.extend(size.to_le_bytes());
    let mut audio = File::create(file).unwrap();
    audio.write_all(&header).unwrap();
    audio.set_len(u64::from(size) + 44).unwrap();
    drop(audio);
    let state = directory.path().join("state");
    binary::initialize(&music, &state);
    let reservation = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = reservation.local_addr().unwrap();
    drop(reservation);
    let log = directory.path().join("logs.jsonl");
    let output = File::create(&log).unwrap();
    let child = Command::new(env!("CARGO_BIN_EXE_hito-plugin-folder"))
        .arg("--music")
        .arg(&music)
        .arg("--state")
        .arg(&state)
        .arg("--listen")
        .arg(address.to_string())
        .env("RUST_LOG", "trace")
        .stdout(Stdio::from(output.try_clone().unwrap()))
        .stderr(Stdio::from(output))
        .spawn()
        .unwrap();
    let mut child = Process(child);
    wait_event(&mut child, &log, "folder_service_ready");
    let page: SearchPage = serde_json::from_value(post(
        address,
        "/v1/search",
        serde_json::json!({"query":"untagged","limit":1}),
    ))
    .unwrap();
    assert_eq!(page.items.len(), 1);
    let sources: SourcePage = serde_json::from_value(post(
        address,
        "/v1/sources",
        serde_json::json!({"recording_id":page.items[0].id}),
    ))
    .unwrap();
    assert_eq!(sources.items.len(), 1);
    let id = sources.items[0]
        .id
        .bytes()
        .map(|byte| format!("%{byte:02X}"))
        .collect::<String>();
    let mut connection = TcpStream::connect(address).unwrap();
    connection
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    let receive = 65536i32;
    assert_eq!(
        unsafe {
            libc::setsockopt(
                std::os::fd::AsRawFd::as_raw_fd(&connection),
                libc::SOL_SOCKET,
                libc::SO_RCVBUF,
                (&receive as *const i32).cast(),
                std::mem::size_of_val(&receive) as libc::socklen_t,
            )
        },
        0
    );
    write!(
        connection,
        "GET /v1/audio?source_id={id} HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n"
    )
    .unwrap();
    let mut headers = Vec::new();
    while !headers.ends_with(b"\r\n\r\n") {
        let mut byte = [0];
        connection.read_exact(&mut byte).unwrap();
        headers.push(byte[0]);
        assert!(headers.len() < 8192);
    }
    assert!(headers.starts_with(b"HTTP/1.1 200"));
    let before = fs::read_to_string(&log).unwrap();
    assert!(
        !before.contains("folder_audio_finished"),
        "response must still be active when SIGTERM arrives"
    );
    assert_eq!(
        unsafe { libc::kill(child.0.id() as libc::pid_t, libc::SIGTERM) },
        0
    );
    wait_event(&mut child, &log, "folder_shutdown_requested");
    assert!(
        child.0.try_wait().unwrap().is_none(),
        "active response must drain before exit"
    );
    let mut received = 0u64;
    let mut buffer = [0; 65536];
    loop {
        let count = connection.read(&mut buffer).unwrap();
        if count == 0 {
            break;
        }
        received += count as u64;
    }
    assert_eq!(received, u64::from(size) + 44);
    drop(connection);
    let deadline = Instant::now() + Duration::from_secs(5);
    let status = loop {
        if let Some(status) = child.0.try_wait().unwrap() {
            break status;
        }
        assert!(
            Instant::now() < deadline,
            "binary did not finish graceful shutdown"
        );
        std::thread::sleep(Duration::from_millis(10));
    };
    assert!(status.success());
    let logs = fs::read_to_string(&log).unwrap();
    let requested = logs.find("folder_shutdown_requested").unwrap();
    let finished = logs.find("folder_audio_finished").unwrap();
    let stopped = logs.find("folder_service_stopped").unwrap();
    assert!(requested < finished && finished < stopped);
    assert!(logs.contains("SIGTERM"));
    assert!(!logs.contains("private-"));
    assert!(TcpStream::connect(address).is_err());
}
fn wait_event(child: &mut Process, path: &std::path::Path, event: &str) {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        if fs::read_to_string(path).unwrap().contains(event) {
            return;
        }
        assert!(
            child.0.try_wait().unwrap().is_none(),
            "binary exited before its expected lifecycle event"
        );
        assert!(Instant::now() < deadline, "lifecycle event timed out");
        std::thread::sleep(Duration::from_millis(10));
    }
}
fn post(address: SocketAddr, path: &str, body: serde_json::Value) -> serde_json::Value {
    let body = body.to_string();
    let mut connection = TcpStream::connect(address).unwrap();
    connection
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    write!(connection,"POST {path} HTTP/1.1\r\nHost: localhost\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",body.len()).unwrap();
    let mut response = Vec::new();
    connection.take(263168).read_to_end(&mut response).unwrap();
    let offset = response
        .windows(4)
        .position(|part| part == b"\r\n\r\n")
        .unwrap()
        + 4;
    assert!(response.starts_with(b"HTTP/1.1 200"));
    serde_json::from_slice(&response[offset..]).unwrap()
}
