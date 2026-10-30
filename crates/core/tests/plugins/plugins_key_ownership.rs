#![cfg(unix)]

use hito_core::test_support::plugin_service as fixture;
use hito_core::test_support::support;
use hito_core::{Actor, CoreConfig, HostDeviceId, InstallPlugin, OfferPluginService, Services};
use std::os::unix::fs::{MetadataExt, PermissionsExt, symlink};
use std::path::{Path, PathBuf};

struct Host {
    server: fixture::PluginServer,
    dir: tempfile::TempDir,
    music: PathBuf,
    actor: Actor,
}

impl Host {
    async fn start() -> Self {
        let server = fixture::PluginServer::start().await;
        let (dir, hito, actor, _library, music) = support::library("plugin-key-ownership").await;
        let service = hito
            .plugins
            .offer_service(
                &actor,
                OfferPluginService {
                    name: "Fixture".into(),
                    base_url: server.url.clone(),
                },
            )
            .await
            .unwrap();
        hito.plugins
            .install(
                &actor,
                InstallPlugin {
                    accept_manifest_digest: None,
                    service_id: service.id,
                    service_revision: 1,
                    bearer_token: Some("ownership-token".into()),
                },
            )
            .await
            .unwrap();
        drop(hito);
        Self {
            server,
            dir,
            music,
            actor,
        }
    }

    fn key(&self) -> PathBuf {
        self.dir.path().join("hito.plugin-key")
    }

    async fn open(&self) -> Services {
        hito_core::open(CoreConfig::new(
            self.dir.path().join("hito.db"),
            self.music.clone(),
            HostDeviceId::new("plugin-key-ownership").unwrap(),
        ))
        .await
        .unwrap()
    }

    async fn ready(&self) -> bool {
        let hito = self.open().await;
        assert!(hito.healthy().await, "local services stay enabled");
        let settings = hito.plugins.settings(&self.actor).await.unwrap();
        assert_eq!(settings.installations.len(), 1);
        assert!(settings.installations[0].credential_set, "rows are kept");
        settings.credential_storage_ready
    }
}

fn mode(path: &Path) -> u32 {
    std::fs::symlink_metadata(path)
        .unwrap()
        .permissions()
        .mode()
        & 0o7777
}

#[tokio::test]
async fn a_key_that_others_can_reach_is_refused_without_being_repaired_or_replaced() {
    let host = Host::start().await;
    assert!(host.ready().await);
    assert_eq!(mode(&host.key()), 0o600);
    let original = std::fs::read(host.key()).unwrap();
    for unsafe_mode in [0o640, 0o604, 0o660, 0o644, 0o777] {
        std::fs::set_permissions(host.key(), std::fs::Permissions::from_mode(unsafe_mode)).unwrap();
        assert!(!host.ready().await, "mode {unsafe_mode:o} must be refused");
        assert_eq!(mode(&host.key()), unsafe_mode, "no silent repair");
        assert_eq!(std::fs::read(host.key()).unwrap(), original);
    }
    std::fs::set_permissions(host.key(), std::fs::Permissions::from_mode(0o600)).unwrap();
    assert!(host.ready().await, "restoring the mode restores storage");
}

#[tokio::test]
async fn a_hard_linked_or_symlinked_key_is_refused() {
    let host = Host::start().await;
    let original = std::fs::read(host.key()).unwrap();
    let alias = host.dir.path().join("elsewhere.copy");
    std::fs::hard_link(host.key(), &alias).unwrap();
    assert!(!host.ready().await, "a second hard link exposes the key");
    std::fs::remove_file(&alias).unwrap();
    assert!(host.ready().await);

    let real = host.dir.path().join("real.key");
    std::fs::rename(host.key(), &real).unwrap();
    symlink(&real, host.key()).unwrap();
    assert!(!host.ready().await, "symlinked keys are refused");
    assert_eq!(std::fs::read(&real).unwrap(), original);
    assert!(std::fs::symlink_metadata(host.key()).unwrap().is_symlink());
}

#[tokio::test]
async fn a_key_owned_by_another_account_is_refused() {
    if unsafe { libc::geteuid() } != 0 {
        eprintln!("skipped: changing ownership needs root; the Linux container runs it");
        return;
    }
    let host = Host::start().await;
    std::os::unix::fs::chown(host.key(), Some(65534), None).unwrap();
    assert!(!host.ready().await);
    assert_eq!(std::fs::metadata(host.key()).unwrap().uid(), 65534);
    std::os::unix::fs::chown(host.key(), Some(0), None).unwrap();
    assert!(host.ready().await);
}

#[tokio::test]
async fn creating_a_key_after_every_credential_is_cleared_publishes_an_owner_only_file_atomically()
{
    let host = Host::start().await;
    let hito = host.open().await;
    let settings = hito.plugins.settings(&host.actor).await.unwrap();
    let installation = &settings.installations[0];
    hito.plugins
        .clear_credential(&host.actor, &installation.id, installation.revision)
        .await
        .unwrap();
    drop(hito);
    std::fs::remove_file(host.key()).unwrap();
    let hito = host.open().await;
    assert!(
        hito.plugins
            .settings(&host.actor)
            .await
            .unwrap()
            .credential_storage_ready
    );
    assert_eq!(mode(&host.key()), 0o600);
    assert_eq!(std::fs::metadata(host.key()).unwrap().nlink(), 1);
    let leftovers: Vec<_> = std::fs::read_dir(host.dir.path())
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .filter(|name| name.starts_with(".tmp"))
        .collect();
    assert!(leftovers.is_empty(), "no temporary key files remain");
    let _ = &host.server;
}

#[tokio::test]
async fn a_data_directory_that_others_can_write_neither_opens_nor_receives_a_key() {
    for unsafe_mode in [0o770, 0o707, 0o777] {
        let dir = tempfile::tempdir().unwrap();
        let music = dir.path().join("music");
        std::fs::create_dir(&music).unwrap();
        std::fs::set_permissions(dir.path(), std::fs::Permissions::from_mode(unsafe_mode)).unwrap();
        let opened = hito_core::open(CoreConfig::new(
            dir.path().join("hito.db"),
            music,
            HostDeviceId::new("plugin-key-ownership").unwrap(),
        ))
        .await;
        assert!(opened.is_err(), "directory mode {unsafe_mode:o}");
        assert!(!dir.path().join("hito.plugin-key").exists());
    }
}
