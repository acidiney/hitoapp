use std::{
    fs::{Metadata, OpenOptions},
    io::Read,
    os::unix::fs::{FileTypeExt, MetadataExt, OpenOptionsExt},
    path::{Path, PathBuf},
};

#[derive(PartialEq, Eq)]
enum Contents {
    File(Vec<u8>),
    Link(PathBuf),
    Directory,
    Fifo,
}

pub(super) struct Snapshot(Vec<(PathBuf, Metadata, Contents)>);

impl Snapshot {
    pub(super) fn unchanged(&self, other: &Self) -> bool {
        self.0.len() == other.0.len()
            && self.0.iter().zip(&other.0).all(
                |((path, before, bytes), (after_path, after, after_bytes))| {
                    path == after_path
                        && identity(before) == identity(after)
                        && bytes == after_bytes
                },
            )
    }

    pub(super) fn capture(directory: &Path) -> Self {
        let mut paths = vec![directory.to_path_buf()];
        let mut entries = Vec::new();
        let mut total = 0;
        while entries.len() < paths.len() {
            assert!(
                paths.len() <= 16,
                "bootstrap snapshot must bound its entry count"
            );
            let path = paths[entries.len()].clone();
            let depth = path
                .strip_prefix(directory)
                .expect("snapshot must stay owned")
                .components()
                .count();
            assert!(
                depth <= 2,
                "bootstrap snapshot must bound its directory depth"
            );
            let metadata = path
                .symlink_metadata()
                .expect("the owned snapshot entry must identify");
            let contents = if metadata.is_symlink() {
                Contents::Link(std::fs::read_link(&path).expect("the owned link must identify"))
            } else if metadata.file_type().is_fifo() {
                Contents::Fifo
            } else if metadata.is_file() {
                assert!(
                    metadata.len() <= 1_048_576,
                    "bootstrap snapshot must bound file bytes"
                );
                let mut file = OpenOptions::new()
                    .read(true)
                    .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
                    .open(&path)
                    .expect("the owned regular snapshot file must open");
                assert!(identity(&file.metadata().unwrap()) == identity(&metadata));
                let mut bytes = Vec::new();
                (&mut file)
                    .take(1_048_577)
                    .read_to_end(&mut bytes)
                    .expect("the owned snapshot file must read within its bound");
                total += bytes.len();
                assert!(
                    bytes.len() as u64 == metadata.len() && total <= 1_048_576,
                    "the complete bootstrap snapshot must stay bounded"
                );
                Contents::File(bytes)
            } else {
                assert!(
                    metadata.is_dir(),
                    "bootstrap snapshot must reject unexpected types"
                );
                let mut children = std::fs::read_dir(&path)
                    .expect("the owned directory must enumerate")
                    .take(17)
                    .map(|entry| {
                        entry
                            .expect("the owned directory entry must identify")
                            .path()
                    })
                    .collect::<Vec<_>>();
                assert!(
                    paths.len() + children.len() <= 16,
                    "bootstrap snapshot must bound directory entries"
                );
                children.sort();
                paths.extend(children);
                Contents::Directory
            };
            entries.push((path, metadata, contents));
        }
        Self(entries)
    }
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
