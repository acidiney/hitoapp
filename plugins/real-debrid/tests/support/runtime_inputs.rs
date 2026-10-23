#[path = "audio.rs"]
mod audio;

use serde_json::{Value, json};
use std::{
    fs::{Metadata, OpenOptions},
    io::Write,
    os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt},
    path::{Path, PathBuf},
};

pub const PROVIDER_TOKEN: &str = "private-runtime-provider-token-canary";
pub const SERVICE_TOKEN: &str = "private-runtime-service-token-canary";
pub const WRONG_TOKEN: &str = "private-runtime-wrong-token-canary";
pub const RECORDING: &str = "private-runtime-recording-id";
pub const TITLE: &str = "private-runtime-recording-title";

pub struct Inputs {
    pub provider: PathBuf,
    pub service: PathBuf,
    pub catalog: PathBuf,
    pub recording: Value,
    files: Vec<(PathBuf, Vec<u8>, Metadata)>,
}

impl Inputs {
    pub fn create(directory: &Path) -> Self {
        std::fs::set_permissions(directory, std::fs::Permissions::from_mode(0o700))
            .expect("runtime directory protection failed");
        let wav = audio::wav();
        audio::assert_decoded(wav.clone());
        let recording = json!({
            "id": RECORDING, "title": TITLE, "artist": "private-runtime-recording-artist",
            "duration_ms": 2_000, "release": null, "disc_number": null,
            "track_number": null, "revision": "private-runtime-catalog-revision",
        });
        let catalog = json!({
            "media_origins": ["https://media.hito-runtime.invalid"],
            "recordings": [{
                "recording": recording, "source_id": "private-runtime-source-id",
                "label": "private-runtime-canonical-wav-label",
                "binding": {
                    "torrent_id": "private-runtime-torrent-id",
                    "info_hash": "0123456789abcdef0123456789abcdef01234567",
                    "file_id": 7, "file_path": "/private-runtime-folder/private-runtime-audio.wav",
                    "file_bytes": wav.len(),
                },
            }],
        });
        let mut files = Vec::new();
        for (name, bytes) in [
            (
                "private-runtime-provider-token",
                format!("{PROVIDER_TOKEN}\n").into_bytes(),
            ),
            (
                "private-runtime-service-token",
                format!("{SERVICE_TOKEN}\n").into_bytes(),
            ),
            (
                "private-runtime-catalog.json",
                serde_json::to_vec(&catalog).unwrap(),
            ),
            ("private-runtime-audio.wav", wav),
        ] {
            let path = directory.join(name);
            let mut file = OpenOptions::new()
                .write(true)
                .create_new(true)
                .mode(0o600)
                .open(&path)
                .expect("runtime operator input setup failed");
            file.write_all(&bytes)
                .expect("runtime operator input write failed");
            let metadata = file.metadata().expect("runtime input metadata failed");
            files.push((path, bytes, metadata));
        }
        Self {
            provider: files[0].0.clone(),
            service: files[1].0.clone(),
            catalog: files[2].0.clone(),
            recording,
            files,
        }
    }

    pub fn assert_preserved(&self, directory: &Path) {
        assert_eq!(
            directory.metadata().unwrap().permissions().mode() & 0o777,
            0o700
        );
        for (path, bytes, before) in &self.files {
            assert!(
                std::fs::read(path).unwrap() == *bytes,
                "runtime changed operator input bytes"
            );
            let after = path.symlink_metadata().unwrap();
            assert!(after.is_file(), "runtime replaced an operator input");
            let identity = |metadata: &Metadata| {
                (
                    metadata.dev(),
                    metadata.ino(),
                    metadata.mode(),
                    metadata.uid(),
                    metadata.gid(),
                    metadata.nlink(),
                    metadata.len(),
                    metadata.mtime(),
                    metadata.mtime_nsec(),
                    metadata.ctime(),
                    metadata.ctime_nsec(),
                )
            };
            assert_eq!(
                identity(&after),
                identity(before),
                "runtime changed operator input metadata"
            );
            assert_eq!(after.permissions().mode() & 0o777, 0o600);
        }
    }
}
