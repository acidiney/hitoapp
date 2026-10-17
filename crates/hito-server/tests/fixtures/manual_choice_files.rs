use std::{
    fs,
    io::ErrorKind,
    path::{Path, PathBuf},
    time::Duration,
};
use uuid::Uuid;

pub(super) struct ManagedCopy {
    directory: PathBuf,
    name: String,
}

struct OwnedFile {
    name: String,
    bytes: Vec<u8>,
}

impl ManagedCopy {
    pub(super) fn capture(directory: &Path, expected: &[u8]) -> Self {
        let directory = directory.join("hito.downloads");
        let files = read_files(&directory);
        assert!(
            files.len() == 1,
            "the initial explicit A download must own exactly one managed file"
        );
        let file = &files[0];
        assert!(
            file.bytes == expected,
            "the initial managed file must contain the exact A bytes"
        );
        Self {
            directory,
            name: file.name.clone(),
        }
    }

    pub(super) fn assert_retained(&self, expected: &[u8]) {
        let files = read_files(&self.directory);
        assert!(
            files.len() == 1 && files[0].name == self.name && files[0].bytes == expected,
            "a source choice and rejected B download must preserve the existing managed A file and bytes"
        );
    }

    pub(super) async fn assert_replaced(&self, expected: &[u8]) {
        let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
        loop {
            let files = read_files(&self.directory);
            if files.len() == 1 && files[0].name != self.name && files[0].bytes == expected {
                return;
            }
            assert!(
                tokio::time::Instant::now() < deadline,
                "a completed explicit B download must leave only its exact managed bytes and retire the superseded A file"
            );
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    }
}

fn read_files(directory: &Path) -> Vec<OwnedFile> {
    let entries = fs::read_dir(directory)
        .unwrap_or_else(|_| panic!("the owned managed directory must remain readable"));
    let mut files = Vec::new();
    for entry in entries {
        let entry =
            entry.unwrap_or_else(|_| panic!("the owned managed entry must remain readable"));
        let kind = match entry.file_type() {
            Ok(kind) => kind,
            Err(error) if error.kind() == ErrorKind::NotFound => continue,
            Err(_) => panic!("the owned managed entry type must remain readable"),
        };
        assert!(
            kind.is_file(),
            "managed downloads must contain only regular owned files"
        );
        let name = entry
            .file_name()
            .into_string()
            .unwrap_or_else(|_| panic!("managed filenames must use the generated UUID format"));
        let parts: Vec<&str> = name.split('.').collect();
        assert!(
            parts.len() == 3
                && parts[2] == "media"
                && parts[..2].iter().all(|part| {
                    Uuid::parse_str(part).is_ok_and(|id| !id.is_nil() && id.to_string() == *part)
                }),
            "completed managed filenames must contain two nonnil canonical UUIDs and the media extension"
        );
        let bytes = match fs::read(entry.path()) {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == ErrorKind::NotFound => continue,
            Err(_) => panic!("the owned managed audio file must remain readable"),
        };
        files.push(OwnedFile { name, bytes });
    }
    files
}
