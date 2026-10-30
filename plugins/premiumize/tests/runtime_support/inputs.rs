use hito_debrid_premiumize::{TransferBinding, TransferFileBinding};
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

pub const PROVIDER_TOKEN: &str = "owned-private-premiumize-runtime-provider-token";
pub const SERVICE_TOKEN: &str = "owned-private-premiumize-runtime-service-token";
pub const WRONG_TOKEN: &str = "owned-private-premiumize-runtime-wrong-token";
pub const RECORDING: &str = "owned-private-premiumize-runtime-recording";
pub const TITLE: &str = "owned-private-premiumize-runtime-title";

pub struct Inputs {
    pub provider: PathBuf,
    pub service: PathBuf,
    pub receipt: PathBuf,
    pub catalog: PathBuf,
    pub recording: ProviderRecording,
    files: Vec<(PathBuf, Vec<u8>, Metadata)>,
}

impl Inputs {
    pub fn create(directory: &Path) -> Self {
        std::fs::set_permissions(directory, std::fs::Permissions::from_mode(0o700))
            .unwrap_or_else(|_| panic!("the owned runtime directory must be private"));
        let recording = ProviderRecording {
            isrc: None,
            id: RECORDING.into(),
            title: TITLE.into(),
            artist: "owned-private-premiumize-runtime-artist".into(),
            duration_ms: Some(2_000),
            release: None,
            disc_number: None,
            track_number: None,
            revision: Some("owned-private-premiumize-runtime-revision".into()),
            attribution: None,
        };
        assert!(
            recording.validate().is_ok(),
            "the owned recording must be SDK-valid"
        );
        let binding = TransferFileBinding {
            transfer: TransferBinding {
                transfer_id: "owned-private-premiumize-runtime-transfer".into(),
                created_at_unix_seconds: 1_700_000_000,
            },
            item_id: "owned-private-premiumize-runtime-item".into(),
            item_created_at_unix_seconds: 1_700_000_120,
            file_name: "owned-private-premiumize-runtime.wav".into(),
            file_bytes: 384_044,
            folder_id: "owned-private-premiumize-runtime-folder".into(),
        };
        assert!(
            binding.validate().is_ok(),
            "the owned full file binding must be valid"
        );
        let catalog = json!({
            "media_origins":["https://media.hito-runtime.invalid"],
            "recordings":[{
                "recording":recording,"source_id":"owned-private-premiumize-runtime-source",
                "label":"owned-private-premiumize-runtime-label","binding":binding
            }]
        });
        let catalog_name =
            OsString::from_vec(b"owned-private-premiumize-runtime-catalog-\xff.json".to_vec());
        let mut files = Vec::new();
        for (name, bytes, mode) in [
            (
                OsString::from("owned-private-premiumize-runtime-provider"),
                format!("{PROVIDER_TOKEN}\r\n").into_bytes(),
                0o400,
            ),
            (
                OsString::from("owned-private-premiumize-runtime-service"),
                format!("{SERVICE_TOKEN}\n").into_bytes(),
                0o600,
            ),
            (
                OsString::from("owned-private-premiumize-runtime-receipt"),
                vec![0x6b; 32],
                0o400,
            ),
            (
                catalog_name,
                serde_json::to_vec(&catalog)
                    .unwrap_or_else(|_| panic!("the owned catalog must encode")),
                0o600,
            ),
        ] {
            let path = directory.join(name);
            let mut file = OpenOptions::new()
                .write(true)
                .create_new(true)
                .mode(mode)
                .open(&path)
                .unwrap_or_else(|_| panic!("the owned private input must create"));
            file.write_all(&bytes)
                .unwrap_or_else(|_| panic!("the owned private input must write"));
            let metadata = file
                .metadata()
                .unwrap_or_else(|_| panic!("the owned private input must identify"));
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
        let directory_mode = directory
            .metadata()
            .unwrap_or_else(|_| panic!("the owned directory must remain"))
            .mode()
            & 0o7777;
        assert_eq!(directory_mode, 0o700);
        assert_eq!(
            std::fs::read_dir(directory)
                .unwrap_or_else(|_| panic!("owned inputs must remain enumerable"))
                .count(),
            4
        );
        for (path, bytes, before) in &self.files {
            let after = path
                .symlink_metadata()
                .unwrap_or_else(|_| panic!("the owned private input must remain"));
            assert!(
                after.is_file(),
                "the runtime must preserve private regular files"
            );
            assert!(
                std::fs::read(path)
                    .ok()
                    .is_some_and(|after| after == *bytes),
                "the runtime must preserve exact private input bytes"
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
                "the runtime must preserve private input identity and metadata"
            );
        }
    }
}
