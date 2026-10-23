use serde_json::json;
use std::{
    fs::{Metadata, OpenOptions},
    io::Write,
    os::unix::fs::{FileTypeExt, MetadataExt, OpenOptionsExt, PermissionsExt},
    path::{Path, PathBuf},
};

pub const PROVIDER_TOKEN: &str = "owned-private-controls-provider-token";
pub const SERVICE_TOKEN: &str = "owned-private-controls-service-token";

pub struct Fixture {
    pub directory: tempfile::TempDir,
    pub provider: PathBuf,
    pub service: PathBuf,
    pub receipt: PathBuf,
    pub catalog: PathBuf,
}

impl Fixture {
    pub fn new() -> Self {
        let directory = tempfile::tempdir().unwrap();
        std::fs::set_permissions(directory.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
        let provider = directory.path().join("owned-private-controls-provider");
        let service = directory.path().join("owned-private-controls-service");
        let receipt = directory.path().join("owned-private-controls-receipt");
        let catalog = directory.path().join("owned-private-controls-catalog.json");
        let contents = json!({
            "media_origins":["https://media.hito-controls.invalid"],
            "recordings":[{
                "recording":{
                    "id":"owned-private-controls-recording","title":"owned-private-controls-title",
                    "artist":"owned-private-controls-artist","duration_ms":2_000,"release":null,
                    "disc_number":null,"track_number":null,"revision":"owned-private-controls-revision"
                },
                "source_id":"owned-private-controls-source","label":"owned-private-controls-label",
                "binding":{
                    "magnet_id":7123,"info_hash":"0123456789abcdef0123456789abcdef01234567",
                    "magnet_bytes":384_044,"file_bytes":384_044,
                    "file_path":["owned-private-controls-folder","owned-private-controls.wav"]
                }
            }]
        });
        for (path, bytes) in [
            (&provider, format!("{PROVIDER_TOKEN}\n").into_bytes()),
            (&service, format!("{SERVICE_TOKEN}\n").into_bytes()),
            (&receipt, vec![0x6b; 32]),
            (&catalog, serde_json::to_vec(&contents).unwrap()),
        ] {
            let mut file = OpenOptions::new()
                .write(true)
                .create_new(true)
                .mode(0o600)
                .open(path)
                .unwrap();
            file.write_all(&bytes).unwrap();
        }
        Self {
            directory,
            provider,
            service,
            receipt,
            catalog,
        }
    }

    pub fn paths(&self) -> Vec<PathBuf> {
        vec![
            self.provider.clone(),
            self.service.clone(),
            self.receipt.clone(),
            self.catalog.clone(),
        ]
    }
}

enum Contents {
    File(Vec<u8>),
    Link(PathBuf),
    Fifo,
    Directory,
}

pub struct Snapshot(Vec<(PathBuf, Metadata, Contents)>);

impl Snapshot {
    pub fn capture(paths: &[PathBuf]) -> Self {
        Self(
            paths
                .iter()
                .map(|path| {
                    let metadata = path.symlink_metadata().unwrap();
                    let contents = if metadata.is_file() {
                        Contents::File(std::fs::read(path).unwrap())
                    } else if metadata.is_symlink() {
                        Contents::Link(std::fs::read_link(path).unwrap())
                    } else if metadata.file_type().is_fifo() {
                        Contents::Fifo
                    } else {
                        assert!(
                            metadata.is_dir(),
                            "the owned control type must be recognized"
                        );
                        assert_eq!(std::fs::read_dir(path).unwrap().count(), 0);
                        Contents::Directory
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
            let after = path.symlink_metadata().unwrap();
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
                "bootstrap must preserve private input metadata"
            );
            match contents {
                Contents::File(bytes) => assert!(
                    std::fs::read(path).unwrap() == *bytes,
                    "bootstrap must preserve private input bytes"
                ),
                Contents::Link(target) => assert!(
                    std::fs::read_link(path).unwrap() == *target,
                    "bootstrap must preserve the owned symlink"
                ),
                Contents::Fifo => assert!(after.file_type().is_fifo()),
                Contents::Directory => {
                    assert!(after.is_dir());
                    assert_eq!(std::fs::read_dir(path).unwrap().count(), 0);
                }
            }
        }
    }
}
