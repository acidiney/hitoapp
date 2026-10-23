use serde_json::{Value, json};
use std::{
    fs::{Metadata, OpenOptions},
    io::Write,
    os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt},
    path::{Path, PathBuf},
};

pub const PROVIDER_TOKEN: &str = "owned-private-bootstrap-provider-token";
pub const SERVICE_TOKEN: &str = "owned-private-bootstrap-service-token";
pub const WRONG_TOKEN: &str = "owned-private-bootstrap-wrong-token";
pub const RECORDING: &str = "owned-private-bootstrap-recording";
pub const TITLE: &str = "owned-private-bootstrap-title";

pub struct Inputs {
    pub provider: PathBuf,
    pub service: PathBuf,
    pub receipt: PathBuf,
    pub catalog: PathBuf,
    pub recording: Value,
    files: Vec<(PathBuf, Vec<u8>, Metadata)>,
}

impl Inputs {
    pub fn create(directory: &Path) -> Self {
        std::fs::set_permissions(directory, std::fs::Permissions::from_mode(0o700)).unwrap();
        let recording = json!({
            "id":RECORDING,"title":TITLE,"artist":"owned-private-bootstrap-artist",
            "duration_ms":2_000,"release":null,"disc_number":null,"track_number":null,
            "revision":"owned-private-bootstrap-revision"
        });
        let catalog = json!({
            "media_origins":["https://media.hito-bootstrap.invalid"],
            "recordings":[{
                "recording":recording,"source_id":"owned-private-bootstrap-source",
                "label":"owned-private-bootstrap-label",
                "binding":{
                    "magnet_id":7123,"info_hash":"0123456789abcdef0123456789abcdef01234567",
                    "magnet_bytes":384_044,"file_path":["owned-private-bootstrap-folder","owned-private-bootstrap.wav"],
                    "file_bytes":384_044
                }
            }]
        });
        let mut files = Vec::new();
        for (name, bytes, mode) in [
            (
                "owned-private-bootstrap-provider",
                format!("{PROVIDER_TOKEN}\r\n").into_bytes(),
                0o400,
            ),
            (
                "owned-private-bootstrap-service",
                format!("{SERVICE_TOKEN}\n").into_bytes(),
                0o600,
            ),
            ("owned-private-bootstrap-receipt", vec![0x53; 32], 0o400),
            (
                "owned-private-bootstrap-catalog.json",
                serde_json::to_vec(&catalog).unwrap(),
                0o600,
            ),
        ] {
            let path = directory.join(name);
            let mut file = OpenOptions::new()
                .write(true)
                .create_new(true)
                .mode(mode)
                .open(&path)
                .unwrap();
            file.write_all(&bytes).unwrap();
            let metadata = file.metadata().unwrap();
            files.push((path, bytes, metadata));
        }
        Self {
            provider: files[0].0.clone(),
            service: files[1].0.clone(),
            receipt: files[2].0.clone(),
            catalog: files[3].0.clone(),
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
            let after = path.symlink_metadata().unwrap();
            assert!(after.is_file());
            assert!(
                std::fs::read(path).unwrap() == *bytes,
                "runtime must preserve private input bytes"
            );
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
                identity(before),
                identity(&after),
                "runtime must preserve private input metadata"
            );
        }
    }
}
