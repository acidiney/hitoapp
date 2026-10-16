use std::{
    fs,
    io::{Read, Seek, SeekFrom},
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
fn production_log_bridge_never_emits_private_parser_diagnostics() {
    let directory = tempfile::tempdir().unwrap();
    let wave = directory.path().join("private-id3-file.wav");
    generate(&wave, "pcm_s16le");
    let mut bytes = fs::read(&wave).unwrap();
    let mut frames = Vec::new();
    for (id, value) in [
        (b"TRCK", "private-id3-track-sentinel"),
        (b"TPOS", "private-id3-disc-sentinel"),
    ] {
        let value = format!("\x03{value}");
        frames.extend(id);
        frames.extend((value.len() as u32).to_be_bytes());
        frames.extend([0, 0]);
        frames.extend(value.bytes());
    }
    let size = frames.len() as u32;
    let mut tag = b"ID3\x03\0\0".to_vec();
    tag.extend([
        (size >> 21) as u8 & 127,
        (size >> 14) as u8 & 127,
        (size >> 7) as u8 & 127,
        size as u8 & 127,
    ]);
    tag.extend(frames);
    bytes.extend(b"id3 ");
    bytes.extend((tag.len() as u32).to_le_bytes());
    bytes.extend(&tag);
    if tag.len() % 2 == 1 {
        bytes.push(0);
    }
    let size = (bytes.len() - 8) as u32;
    bytes[4..8].copy_from_slice(&size.to_le_bytes());
    fs::write(&wave, bytes).unwrap();
    malformed_ape(directory.path());
    malformed_ogg(directory.path());
    for setting in [
        "trace",
        "trace,lofty::id3::v2::tag=trace,lofty::ape=trace,lofty::ogg=trace",
    ] {
        let state_parent = tempfile::tempdir().unwrap();
        let state = state_parent.path().join("state");
        binary::initialize(directory.path(), &state);
        let mut output = tempfile::tempfile().unwrap();
        let child = Command::new(env!("CARGO_BIN_EXE_hito-plugin-folder"))
            .args(["--music"])
            .arg(directory.path())
            .arg("--state")
            .arg(&state)
            .args(["--listen", "127.0.0.1:0"])
            .env("RUST_LOG", setting)
            .stdout(Stdio::from(output.try_clone().unwrap()))
            .stderr(Stdio::from(output.try_clone().unwrap()))
            .spawn()
            .unwrap();
        let mut child = Process(child);
        let deadline = Instant::now() + Duration::from_secs(5);
        let mut logs = String::new();
        loop {
            output.seek(SeekFrom::Start(0)).unwrap();
            logs.clear();
            output.read_to_string(&mut logs).unwrap();
            if logs.contains("folder_service_ready") {
                break;
            }
            assert!(
                child.0.try_wait().unwrap().is_none(),
                "service stopped before scanning"
            );
            assert!(Instant::now() < deadline, "service startup timed out");
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(logs.contains("folder_scan_complete"));
        assert!(
            !logs.contains("private-"),
            "production logging exposed private metadata"
        );
    }
}
fn generate(path: &std::path::Path, codec: &str) {
    let status = Command::new("ffmpeg")
        .args([
            "-v",
            "error",
            "-f",
            "lavfi",
            "-i",
            "sine=frequency=440:sample_rate=8000",
            "-t",
            "0.2",
            "-c:a",
            codec,
        ])
        .arg(path)
        .status()
        .unwrap();
    assert!(status.success());
}
fn malformed_ape(root: &std::path::Path) {
    let path = root.join("private-ape-file.mp3");
    generate(&path, "libmp3lame");
    let mut bytes = fs::read(&path).unwrap();
    let mut item = Vec::new();
    item.extend(0u32.to_le_bytes());
    item.extend(0u32.to_le_bytes());
    item.extend(b"private-ape-key-sentinel\0");
    let mut footer = b"APETAGEX".to_vec();
    footer.extend(2000u32.to_le_bytes());
    footer.extend(((item.len() + 32) as u32).to_le_bytes());
    footer.extend(1u32.to_le_bytes());
    footer.extend([0u8; 12]);
    bytes.extend(item);
    bytes.extend(footer);
    fs::write(path, bytes).unwrap();
}
fn malformed_ogg(root: &std::path::Path) {
    let path = root.join("private-ogg-file.ogg");
    generate(&path, "libvorbis");
    let mut bytes = fs::read(&path).unwrap();
    let start = bytes
        .windows(7)
        .position(|part| part == b"\x03vorbis")
        .unwrap()
        + 7;
    let length = u32::from_le_bytes(bytes[start..start + 4].try_into().unwrap()) as usize;
    assert!(length >= 12);
    let vendor = &mut bytes[start + 4..start + 4 + length];
    vendor.fill(0xff);
    vendor[..11].copy_from_slice(b"private-ogg");
    let mut offset = 0;
    while offset < bytes.len() {
        assert_eq!(&bytes[offset..offset + 4], b"OggS");
        let segments = bytes[offset + 26] as usize;
        let length = bytes[offset + 27..offset + 27 + segments]
            .iter()
            .map(|value| *value as usize)
            .sum::<usize>();
        let end = offset + 27 + segments + length;
        bytes[offset + 22..offset + 26].fill(0);
        let mut crc = 0u32;
        for byte in &bytes[offset..end] {
            crc ^= u32::from(*byte) << 24;
            for _ in 0..8 {
                crc = if crc & 0x80000000 != 0 {
                    (crc << 1) ^ 0x04c11db7
                } else {
                    crc << 1
                };
            }
        }
        bytes[offset + 22..offset + 26].copy_from_slice(&crc.to_le_bytes());
        offset = end;
    }
    fs::write(path, bytes).unwrap();
}
#[allow(dead_code)]
#[path = "support/binary.rs"]
mod binary;
