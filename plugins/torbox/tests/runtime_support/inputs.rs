use hito_debrid_torbox::TorrentFileBinding;
use hito_plugin_sdk::ProviderRecording;
use serde_json::json;
use std::{
    ffi::OsString,
    fs::{Metadata, OpenOptions},
    io::Write,
    os::unix::{
        ffi::OsStringExt,
        fs::{MetadataExt, OpenOptionsExt, PermissionsExt},
    },
    path::{Path, PathBuf},
};

pub const PROVIDER_TOKEN: &str = "owned-private-torbox-runtime-provider-token";
pub const SERVICE_TOKEN: &str = "owned-private-torbox-runtime-service-token";
pub const WRONG_TOKEN: &str = "owned-private-torbox-runtime-wrong-token";
pub const RECORDING: &str = "owned-private-torbox-runtime-recording";
pub const TITLE: &str = "owned-private-torbox-runtime-title";

pub struct Inputs {
    pub provider: PathBuf,
    pub service: PathBuf,
    pub key: PathBuf,
    pub catalog: PathBuf,
    pub state: PathBuf,
    pub recording: ProviderRecording,
    files: Vec<(PathBuf, Vec<u8>, Metadata)>,
}

impl Inputs {
    pub fn arguments(&self, mode: &str) -> Vec<OsString> {
        vec![
            "--provider-token-file".into(),
            self.provider.as_os_str().into(),
            "--service-token-file".into(),
            self.service.as_os_str().into(),
            "--state-key-file".into(),
            self.key.as_os_str().into(),
            "--state-directory".into(),
            self.state.as_os_str().into(),
            "--state-mode".into(),
            mode.into(),
            "--catalog-file".into(),
            self.catalog.as_os_str().into(),
        ]
    }

    pub fn create(directory: &Path) -> Self {
        std::fs::set_permissions(directory, std::fs::Permissions::from_mode(0o700))
            .expect("the owned input directory must be private");
        let recording = ProviderRecording {
            isrc: None,
            artist_urls: Vec::new(),
            artists: Vec::new(),
            id: RECORDING.into(),
            title: TITLE.into(),
            artist: "owned-private-torbox-runtime-artist".into(),
            duration_ms: Some(2_000),
            release: None,
            disc_number: None,
            track_number: None,
            revision: Some("owned-private-torbox-runtime-revision".into()),
            attribution: None,
        };
        assert!(recording.validate().is_ok());
        let binding = TorrentFileBinding {
            torrent_id: 7123,
            info_hash: "0123456789abcdef0123456789abcdef01234567".into(),
            torrent_bytes: 384_044,
            file_id: 7,
            file_name: "owned-private-torbox-runtime.wav".into(),
            file_bytes: 384_044,
        };
        assert!(binding.validate().is_ok());
        let catalog = json!({
            "media_origins":["https://media.hito-runtime.invalid"],
            "recordings":[{"recording":recording,
                "source_id":"owned-private-torbox-runtime-source",
                "label":"owned-private-torbox-runtime-label","binding":binding}]
        });
        let catalog_name =
            OsString::from_vec(b"owned-private-torbox-runtime-catalog-\xff.json".to_vec());
        let mut files = Vec::new();
        for (name, bytes, mode) in [
            (
                OsString::from("owned-private-torbox-runtime-provider"),
                format!("{PROVIDER_TOKEN}\r\n").into_bytes(),
                0o400,
            ),
            (
                OsString::from("owned-private-torbox-runtime-service"),
                format!("{SERVICE_TOKEN}\n").into_bytes(),
                0o600,
            ),
            (
                OsString::from("owned-private-torbox-runtime-key"),
                vec![0x6b; 32],
                0o600,
            ),
            (
                catalog_name,
                serde_json::to_vec(&catalog).expect("the owned catalog must encode"),
                0o600,
            ),
        ] {
            let path = directory.join(name);
            let mut file = OpenOptions::new()
                .write(true)
                .create_new(true)
                .mode(mode)
                .open(&path)
                .expect("the owned private input must create");
            file.write_all(&bytes)
                .expect("the owned private input must write");
            let metadata = file
                .metadata()
                .expect("the owned private input must identify");
            files.push((path, bytes, metadata));
        }
        Self {
            provider: files[0].0.clone(),
            service: files[1].0.clone(),
            key: files[2].0.clone(),
            catalog: files[3].0.clone(),
            state: directory.join("journal"),
            recording,
            files,
        }
    }

    pub fn assert_preserved(&self, directory: &Path) {
        assert_eq!(
            directory
                .metadata()
                .expect("the owned directory must remain")
                .mode()
                & 0o7777,
            0o700
        );
        assert_eq!(
            std::fs::read_dir(directory)
                .expect("owned inputs must remain enumerable")
                .count(),
            4 + usize::from(self.state.exists())
        );
        for (path, bytes, before) in &self.files {
            let after = path
                .symlink_metadata()
                .expect("the owned private input must remain");
            assert!(after.is_file());
            assert!(
                std::fs::read(path)
                    .ok()
                    .is_some_and(|after| after == *bytes),
                "runtime must preserve exact operator input bytes"
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
            assert!(
                identity(before) == identity(&after),
                "runtime must preserve operator input metadata"
            );
        }
    }

    pub fn database_identity(&self) -> (u64, u64) {
        let directory = self
            .state
            .symlink_metadata()
            .expect("explicit creation must retain state");
        assert!(directory.is_dir() && directory.mode() & 0o7777 == 0o700);
        let database = self
            .state
            .join("preparation.sqlite")
            .symlink_metadata()
            .expect("the real private journal must remain");
        assert!(database.is_file() && database.mode() & 0o7777 == 0o600);
        assert_eq!(database.nlink(), 1);
        (database.dev(), database.ino())
    }
}
