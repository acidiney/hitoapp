use serde_json::{Value, json};
use std::{
    fs::{Metadata, OpenOptions},
    io::Write,
    os::unix::fs::{FileTypeExt, MetadataExt, OpenOptionsExt, PermissionsExt},
    path::{Path, PathBuf},
};

pub const PROVIDER_TOKEN: &str = "private-bootstrap-provider-token-canary";

pub struct Fixture {
    pub directory: tempfile::TempDir,
    pub provider: PathBuf,
    pub service: PathBuf,
    pub catalog: PathBuf,
}

impl Fixture {
    pub fn new() -> Self {
        let directory = tempfile::tempdir().expect("bootstrap directory setup failed");
        std::fs::set_permissions(directory.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
        let provider = directory.path().join("private-bootstrap-provider-token");
        let service = directory.path().join("private-bootstrap-service-token");
        let catalog = directory.path().join("private-bootstrap-catalog.json");
        for (path, bytes) in [
            (&provider, format!("{PROVIDER_TOKEN}\n").into_bytes()),
            (
                &service,
                b"private-bootstrap-service-token-canary\n".to_vec(),
            ),
            (&catalog, serde_json::to_vec(&valid_catalog()).unwrap()),
        ] {
            let mut file = OpenOptions::new()
                .write(true)
                .create_new(true)
                .mode(0o600)
                .open(path)
                .expect("bootstrap operator file setup failed");
            file.write_all(&bytes)
                .expect("bootstrap operator file write failed");
        }
        Self {
            directory,
            provider,
            service,
            catalog,
        }
    }

    pub fn paths(&self) -> Vec<PathBuf> {
        vec![
            self.provider.clone(),
            self.service.clone(),
            self.catalog.clone(),
        ]
    }
}

pub fn valid_catalog() -> Value {
    json!({
        "media_origins": ["https://media.hito-bootstrap.invalid"],
        "recordings": [{
            "recording": {
                "id": "private-bootstrap-recording-id", "title": "private-bootstrap-recording-title",
                "artist": "private-bootstrap-recording-artist", "duration_ms": 2_000,
                "release": null, "disc_number": null, "track_number": null,
                "revision": "private-bootstrap-catalog-revision",
            },
            "source_id": "private-bootstrap-source-id", "label": "private-bootstrap-source-label",
            "binding": {
                "torrent_id": "private-bootstrap-torrent-id",
                "info_hash": "0123456789abcdef0123456789abcdef01234567",
                "file_id": 7, "file_path": "/private-bootstrap-folder/private-bootstrap-audio.wav",
                "file_bytes": 384_044,
            },
        }],
    })
}

enum Contents {
    File(Vec<u8>),
    Link(PathBuf),
    Fifo,
}

pub struct Snapshot(Vec<(PathBuf, Metadata, Contents)>);

impl Snapshot {
    pub fn capture(paths: &[PathBuf]) -> Self {
        Self(
            paths
                .iter()
                .map(|path| {
                    let metadata = path
                        .symlink_metadata()
                        .expect("bootstrap snapshot metadata failed");
                    let contents = if metadata.is_file() {
                        Contents::File(
                            std::fs::read(path).expect("bootstrap snapshot file read failed"),
                        )
                    } else if metadata.is_symlink() {
                        Contents::Link(
                            std::fs::read_link(path).expect("bootstrap snapshot link read failed"),
                        )
                    } else {
                        assert!(
                            metadata.file_type().is_fifo(),
                            "bootstrap fixture type was unexpected"
                        );
                        Contents::Fifo
                    };
                    (path.clone(), metadata, contents)
                })
                .collect(),
        )
    }

    pub fn assert_preserved(&self, directory: &Path) {
        assert_eq!(
            directory.metadata().unwrap().permissions().mode() & 0o777,
            0o700
        );
        for (path, before, contents) in &self.0 {
            let after = path
                .symlink_metadata()
                .expect("bootstrap changed an operator input");
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
                "bootstrap changed operator input metadata"
            );
            match contents {
                Contents::File(bytes) => assert!(
                    std::fs::read(path).unwrap() == *bytes,
                    "bootstrap changed operator input bytes"
                ),
                Contents::Link(target) => assert!(
                    std::fs::read_link(path).unwrap() == *target,
                    "bootstrap changed operator symlink target"
                ),
                Contents::Fifo => assert!(
                    after.file_type().is_fifo(),
                    "bootstrap replaced an operator FIFO"
                ),
            }
        }
    }
}
