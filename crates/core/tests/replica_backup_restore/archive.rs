use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    fs::{self, OpenOptions},
    io::Write,
    os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt},
    path::{Path, PathBuf},
};
use uuid::Uuid;

#[derive(PartialEq, Eq)]
pub struct Entry {
    mode: u32,
    uid: u32,
    gid: u32,
    links: u64,
    bytes: Option<[u8; 32]>,
}

pub fn private_directory(path: &Path) {
    fs::create_dir(path).unwrap();
    fs::set_permissions(path, fs::Permissions::from_mode(0o700)).unwrap();
}

pub fn private_file(path: &Path, bytes: &[u8]) {
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)
        .unwrap();
    file.write_all(bytes).unwrap();
    file.sync_all().unwrap();
}

pub fn snapshot(root: &Path) -> BTreeMap<PathBuf, Entry> {
    let mut entries = BTreeMap::new();
    let mut pending = vec![root.to_path_buf()];
    while let Some(path) = pending.pop() {
        let metadata = fs::symlink_metadata(&path).unwrap();
        assert!(
            metadata.is_file() || metadata.is_dir(),
            "owned archive contains an unexpected file type"
        );
        let bytes = if metadata.is_file() {
            Some(Sha256::digest(fs::read(&path).unwrap()).into())
        } else {
            for entry in fs::read_dir(&path).unwrap() {
                pending.push(entry.unwrap().path());
            }
            None
        };
        entries.insert(
            path.strip_prefix(root).unwrap().to_path_buf(),
            Entry {
                mode: metadata.mode() & 0o777,
                uid: metadata.uid(),
                gid: metadata.gid(),
                links: metadata.nlink(),
                bytes,
            },
        );
    }
    entries
}

pub fn assert_private_data(root: &Path, binding: Uuid) {
    let entries = snapshot(root);
    let owner = fs::metadata(root).unwrap();
    for (path, entry) in &entries {
        assert_eq!(entry.uid, owner.uid());
        assert_eq!(entry.gid, owner.gid());
        assert_eq!(
            entry.mode,
            if entry.bytes.is_some() { 0o600 } else { 0o700 },
            "archive did not preserve private modes"
        );
        if entry.bytes.is_some() {
            assert_eq!(entry.links, 1);
        }
        assert!(!path.is_absolute());
    }
    let key = root.join("hito.plugin-key");
    assert_eq!(fs::metadata(key).unwrap().len(), 32);
    assert!(root.join("hito.db").is_file());
    let profile = root.join("hito.replicas").join(binding.to_string());
    for name in ["owner", "profile.lock", "hito.db"] {
        assert!(profile.join(name).is_file());
    }
}

pub async fn create(source: &Path, archive: &Path) {
    private_file(archive, &[]);
    let result = tokio::process::Command::new("tar")
        .args(["--create", "--file"])
        .arg(archive)
        .arg("--directory")
        .arg(source)
        .arg(".")
        .output()
        .await
        .unwrap();
    assert!(
        result.status.success(),
        "owned stopped-data archive creation failed"
    );
}

pub async fn extract(archive: &Path, destination: &Path) {
    assert!(
        fs::read_dir(destination).unwrap().next().is_none(),
        "restore must use a new empty data directory"
    );
    let result = tokio::process::Command::new("tar")
        .args(["--extract", "--same-owner", "--same-permissions", "--file"])
        .arg(archive)
        .arg("--directory")
        .arg(destination)
        .output()
        .await
        .unwrap();
    assert!(
        result.status.success(),
        "owned stopped-data archive extraction failed"
    );
}
