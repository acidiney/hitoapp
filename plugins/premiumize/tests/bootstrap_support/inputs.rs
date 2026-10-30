use hito_debrid_premiumize::{TransferBinding, TransferFileBinding};
use hito_plugin_sdk::ProviderRecording;
use serde_json::{Value, json};
use std::{
    ffi::OsString,
    fs::{Metadata, OpenOptions},
    io::Write,
    os::unix::{
        ffi::OsStringExt,
        fs::{FileTypeExt, MetadataExt, OpenOptionsExt, PermissionsExt},
    },
    path::{Path, PathBuf},
};

pub const PRIVATE: &str = "owned-private-premiumize-bootstrap-";
pub const PROVIDER_TOKEN: &str = "owned-private-premiumize-bootstrap-provider-token";
pub const SERVICE_TOKEN: &str = "owned-private-premiumize-bootstrap-service-token";

pub struct Fixture {
    pub directory: tempfile::TempDir,
    pub provider: PathBuf,
    pub service: PathBuf,
    pub receipt: PathBuf,
    pub catalog: PathBuf,
}

impl Fixture {
    pub fn new() -> Self {
        let directory = tempfile::Builder::new()
            .prefix(PRIVATE)
            .tempdir()
            .unwrap_or_else(|_| panic!("the owned bootstrap directory must create"));
        std::fs::set_permissions(directory.path(), std::fs::Permissions::from_mode(0o700))
            .unwrap_or_else(|_| panic!("the owned bootstrap directory must be private"));
        let provider = directory.path().join(format!("{PRIVATE}provider"));
        let service = directory.path().join(format!("{PRIVATE}service"));
        let receipt = directory.path().join(format!("{PRIVATE}receipt"));
        let catalog = directory.path().join(OsString::from_vec(
            b"owned-private-premiumize-bootstrap-catalog-\xff.json".to_vec(),
        ));
        for (path, bytes) in [
            (&provider, format!("{PROVIDER_TOKEN}\r\n").into_bytes()),
            (&service, format!("{SERVICE_TOKEN}\n").into_bytes()),
            (&receipt, vec![0x6b; 32]),
            (
                &catalog,
                serde_json::to_vec(&catalog_with(1, 0))
                    .unwrap_or_else(|_| panic!("the owned catalog must encode")),
            ),
        ] {
            let mut file = OpenOptions::new()
                .write(true)
                .create_new(true)
                .mode(0o600)
                .open(path)
                .unwrap_or_else(|_| panic!("the owned bootstrap input must create"));
            file.write_all(&bytes)
                .unwrap_or_else(|_| panic!("the owned bootstrap input must write"));
        }
        Self {
            directory,
            provider,
            service,
            receipt,
            catalog,
        }
    }

    pub fn arguments(&self) -> Vec<OsString> {
        vec![
            "--provider-token-file".into(),
            self.provider.as_os_str().into(),
            "--service-token-file".into(),
            self.service.as_os_str().into(),
            "--receipt-key-file".into(),
            self.receipt.as_os_str().into(),
            "--catalog-file".into(),
            self.catalog.as_os_str().into(),
        ]
    }
}

pub fn catalog_with(count: usize, title_padding: usize) -> Value {
    let recordings = (0..count)
        .map(|index| {
            let recording = ProviderRecording {
                isrc: None,
                id: format!("{PRIVATE}recording-{index}"),
                title: format!("{PRIVATE}title-{index}{}", "x".repeat(title_padding)),
                artist: format!("{PRIVATE}artist"),
                duration_ms: Some(2_000),
                release: None,
                disc_number: None,
                track_number: None,
                revision: Some(format!("{PRIVATE}revision")),
                attribution: None,
            };
            let binding = TransferFileBinding {
                transfer: TransferBinding {
                    transfer_id: format!("{PRIVATE}transfer-{index}"),
                    created_at_unix_seconds: 1_700_000_000,
                },
                item_id: format!("{PRIVATE}item-{index}"),
                item_created_at_unix_seconds: 1_700_000_120,
                file_name: format!("{PRIVATE}audio.wav"),
                file_bytes: 384_044,
                folder_id: format!("{PRIVATE}folder"),
            };
            assert!(
                recording.validate().is_ok() && binding.validate().is_ok(),
                "owned catalog entries must satisfy the public types"
            );
            json!({"recording":recording,"source_id":format!("{PRIVATE}source-{index}"),
            "label":format!("{PRIVATE}label"),"binding":binding})
        })
        .collect::<Vec<_>>();
    json!({"media_origins":["https://media.hito-bootstrap.invalid"],"recordings":recordings})
}

enum Contents {
    File(Vec<u8>),
    Link(PathBuf),
    Fifo,
    Directory,
}

pub struct Snapshot {
    directory: Metadata,
    entries: Vec<(PathBuf, Metadata, Contents)>,
}

impl Snapshot {
    pub fn capture(directory: &Path) -> Self {
        let entries = paths(directory)
            .into_iter()
            .map(|path| {
                let metadata = path
                    .symlink_metadata()
                    .unwrap_or_else(|_| panic!("the owned input must identify"));
                let contents = if metadata.is_file() {
                    Contents::File(
                        std::fs::read(&path)
                            .unwrap_or_else(|_| panic!("the owned input snapshot must read")),
                    )
                } else if metadata.is_symlink() {
                    Contents::Link(
                        std::fs::read_link(&path)
                            .unwrap_or_else(|_| panic!("the owned symlink must identify")),
                    )
                } else if metadata.file_type().is_fifo() {
                    Contents::Fifo
                } else {
                    assert!(
                        metadata.is_dir() && paths(&path).is_empty(),
                        "the owned directory control must be empty"
                    );
                    Contents::Directory
                };
                (path, metadata, contents)
            })
            .collect();
        Self {
            directory: directory
                .metadata()
                .unwrap_or_else(|_| panic!("the owned directory must identify")),
            entries,
        }
    }

    pub fn assert_preserved(&self, directory: &Path) {
        let after = directory
            .metadata()
            .unwrap_or_else(|_| panic!("the owned directory must remain"));
        assert!(
            identity(&self.directory) == identity(&after) && after.mode() & 0o7777 == 0o700,
            "bootstrap must preserve the exact private directory metadata"
        );
        let expected = self
            .entries
            .iter()
            .map(|(path, _, _)| path.clone())
            .collect::<Vec<_>>();
        assert!(
            paths(directory) == expected,
            "bootstrap must preserve the exact operator directory entries"
        );
        for (path, before, contents) in &self.entries {
            let after = path
                .symlink_metadata()
                .unwrap_or_else(|_| panic!("the owned input must remain"));
            assert!(
                identity(before) == identity(&after),
                "bootstrap must preserve exact input metadata"
            );
            match contents {
                Contents::File(bytes) => assert!(
                    std::fs::read(path)
                        .ok()
                        .is_some_and(|after| after == *bytes),
                    "bootstrap must preserve exact private input bytes"
                ),
                Contents::Link(target) => assert!(
                    std::fs::read_link(path)
                        .ok()
                        .is_some_and(|after| after == *target),
                    "bootstrap must preserve the owned symlink target"
                ),
                Contents::Fifo => assert!(
                    after.file_type().is_fifo(),
                    "bootstrap must preserve the owned FIFO"
                ),
                Contents::Directory => assert!(
                    after.is_dir() && paths(path).is_empty(),
                    "bootstrap must preserve the owned empty directory"
                ),
            }
        }
    }
}

fn paths(directory: &Path) -> Vec<PathBuf> {
    let mut paths = std::fs::read_dir(directory)
        .unwrap_or_else(|_| panic!("the owned directory must enumerate"))
        .map(|entry| {
            entry
                .unwrap_or_else(|_| panic!("the owned entry must identify"))
                .path()
        })
        .collect::<Vec<_>>();
    paths.sort();
    paths
}

fn identity(metadata: &Metadata) -> (u64, u64, u32, u32, u32, u64, u64, i64, i64, i64, i64) {
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
}
