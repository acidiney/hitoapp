use super::{
    export::{Export, LIBRARY},
    fixture::Case,
    peer::Remote,
};
use hito_core::BeginCatalogSnapshot;
use std::{
    fs::File,
    io::Write,
    os::unix::fs::MetadataExt,
    path::{Path, PathBuf},
};
use uuid::Uuid;

#[tokio::test]
#[ignore = "requires the isolated 64 MiB tmpfs storage Compose service"]
async fn actual_disk_exhaustion_preserves_old_catalog_and_recovers_after_owned_filler_removal() {
    let root = isolated_tmpfs();
    let sentinel = tempfile::tempdir_in(&root).unwrap();
    let sentinel_path = sentinel.path().join("owned-original-sentinel");
    let sentinel_bytes = b"Owned original outside application data";
    std::fs::write(&sentinel_path, sentinel_bytes).unwrap();
    let case = Case::new(Some(&root)).await;
    let mut remote = Remote::start(401).await;
    let bound = case.bind(&remote).await;
    case.install(&bound).await.unwrap();
    let old_usage = case.disk_usage();
    let replacement = Export {
        id: Uuid::from_u128(711),
        recordings: 8_000,
    };
    remote.replace(replacement);
    remote.hold_page(1);
    let replicas = case.services.replicas.clone();
    let actor = case.actor.clone();
    let session = bound.session.clone();
    let installing = tokio::spawn(async move {
        replicas
            .install_catalog(
                &actor,
                &session,
                BeginCatalogSnapshot {
                    library_id: LIBRARY.into(),
                },
            )
            .await
    });
    remote.wait_for_page().await;
    assert_eq!(
        remote.counts().0,
        1,
        "first page must settle before the next request"
    );
    let filler = Filler::exhaust(sentinel.path());
    let filled_bytes = filler.bytes;
    remote.release_page();
    let error = installing
        .await
        .unwrap()
        .expect_err("a full filesystem cannot install the replacement");
    assert!(
        matches!(error, hito_core::Error::Internal(message) if message == "Private replica storage is unavailable")
    );
    let (pages, encoded_bytes) = remote.counts();
    assert!(pages > 1 && pages < replacement.metadata().page_count as usize);
    case.verify_catalog(&bound, 401).await;
    assert_eq!(std::fs::read(&sentinel_path).unwrap(), sentinel_bytes);
    eprintln!(
        "catalog_storage_disk_failure case=tmpfs filesystem_bytes=67108864 filler_written_bytes={filled_bytes} filler_errno=28 public_error=internal served_pages={pages} encoded_bytes={encoded_bytes} old={old_usage:?} failed={:?}",
        case.disk_usage()
    );
    filler.release();
    remote.pause().await;
    let case = case.reopen().await;
    let bound = case.activate(bound.connection, &bound.client_id).await;
    case.verify_catalog(&bound, 401).await;
    case.verify_owned_state(&bound).await;
    assert_eq!(std::fs::read(&sentinel_path).unwrap(), sentinel_bytes);
    remote.resume().await;
    let recovered = replacement;
    let before_retry = remote.counts();
    let retry_started = std::time::Instant::now();
    assert_eq!(case.install(&bound).await.unwrap(), recovered.metadata());
    let retry_elapsed = retry_started.elapsed();
    let after_retry = remote.counts();
    assert_eq!(after_retry.0 - before_retry.0, 40);
    eprintln!(
        "catalog_storage_retry case=tmpfs recordings=8000 pages=40 encoded_bytes={} elapsed_ms={}",
        after_retry.1 - before_retry.1,
        retry_elapsed.as_millis()
    );
    case.verify_export(&bound, recovered).await;
    assert!(matches!(
        case.services
            .replicas
            .catalog_page(&case.actor, &bound.session, super::export::SNAPSHOT, 0)
            .await,
        Err(hito_core::Error::NotFound)
    ));
    remote.pause().await;
    let case = case.reopen().await;
    let bound = case.activate(bound.connection, &bound.client_id).await;
    case.verify_export(&bound, recovered).await;
    case.verify_owned_state(&bound).await;
    assert_eq!(std::fs::read(&sentinel_path).unwrap(), sentinel_bytes);
    eprintln!(
        "catalog_storage_disk_recovery case=tmpfs recovered_pages=40 disk={:?}",
        case.disk_usage()
    );
    remote.resume().await;
    remote.assert_pending_and_cursor(&case, &bound).await;
    remote.stop().await;
    case.close().await;
}

fn isolated_tmpfs() -> PathBuf {
    let root = std::env::var_os("HITO_CATALOG_DISK_ROOT")
        .expect("set HITO_CATALOG_DISK_ROOT using the isolated storage service");
    let root = std::fs::canonicalize(root).unwrap();
    let mountinfo = std::fs::read_to_string("/proc/self/mountinfo").unwrap();
    let mount = mountinfo
        .lines()
        .find(|line| line.split_whitespace().nth(4) == root.to_str())
        .expect("disk case root must be a dedicated mount");
    assert!(
        mount.split_once(" - ").unwrap().1.starts_with("tmpfs "),
        "only the owned tmpfs may be filled"
    );
    let output = std::process::Command::new("df")
        .args(["-kP"])
        .arg(&root)
        .output()
        .unwrap();
    assert!(output.status.success());
    let sizes: Vec<_> = std::str::from_utf8(&output.stdout)
        .unwrap()
        .lines()
        .nth(1)
        .unwrap()
        .split_whitespace()
        .collect();
    assert_eq!(
        sizes[1], "65536",
        "the isolated fixture must have exactly 64 MiB capacity"
    );
    root
}

struct Filler {
    path: PathBuf,
    bytes: u64,
}
impl Filler {
    fn exhaust(directory: &Path) -> Self {
        let path = directory.join("owned-filler");
        let mut file = File::create_new(&path).unwrap();
        let chunk = [0xa5; 256 * 1024];
        let mut bytes = 0;
        loop {
            match file.write(&chunk) {
                Ok(0) => panic!("filler write made no progress"),
                Ok(written) => bytes += written as u64,
                Err(error) => {
                    assert_eq!(
                        error.raw_os_error(),
                        Some(28),
                        "actual regular-file writes must reach ENOSPC"
                    );
                    break;
                }
            }
        }
        let metadata = file.metadata().unwrap();
        assert!(bytes > 0);
        assert_eq!(metadata.len(), bytes);
        assert!(
            metadata.blocks() * 512 >= bytes,
            "filler must consume allocated blocks"
        );
        drop(file);
        Self { path, bytes }
    }
    fn release(self) {
        std::fs::remove_file(self.path).unwrap();
    }
}
