use super::{
    export::{Export, LIBRARY, SNAPSHOT},
    peer::Remote,
};
use hito_core::{
    Action, Actor, BeginCatalogSnapshot, Browse, CatalogSnapshot, ConnectReplica, CoreConfig,
    Credentials, HostDeviceId, Operation, ReplicaSession, Services, SyncBatch,
};
use serde_json::Value;
use std::{
    collections::HashSet,
    path::{Path, PathBuf},
    time::Instant,
};
use uuid::Uuid;

pub struct Case {
    directory: tempfile::TempDir,
    original: PathBuf,
    original_bytes: Vec<u8>,
    standalone_catalog: Value,
    standalone_events: Value,
    pub services: Services,
    pub actor: Actor,
}
pub struct Bound {
    pub connection: Uuid,
    pub session: ReplicaSession,
    pub client_id: String,
}

fn credentials() -> Credentials {
    Credentials {
        username: "admin".into(),
        password: "owned-storage-password".into(),
    }
}
fn config(path: &Path) -> CoreConfig {
    CoreConfig::new(
        path.join("hito.db"),
        path.join("music"),
        HostDeviceId::new("owned-storage-host").unwrap(),
    )
}
pub fn pending() -> Operation {
    Operation {
        id: Uuid::from_u128(704),
        action: Action::PlaylistRename {
            playlist_id: Uuid::from_u128(702),
            name: "Pending owned rename".into(),
        },
    }
}

impl Case {
    pub async fn new(root: Option<&Path>) -> Self {
        let directory = match root {
            Some(root) => tempfile::tempdir_in(root),
            None => tempfile::tempdir(),
        }
        .unwrap();
        std::fs::create_dir(directory.path().join("music")).unwrap();
        let original = directory.path().join("music").join("owned.wav");
        let original_bytes = hito_core::test_support::support::write_wav(&original, 41);
        let services = hito_core::open(config(directory.path())).await.unwrap();
        let login = services.identity.setup(credentials()).await.unwrap();
        let actor = services.identity.authenticate(&login.token).await.unwrap();
        let library = services
            .catalog
            .libraries(&actor)
            .await
            .unwrap()
            .remove(0)
            .id;
        hito_core::test_support::support::scan(&services, &actor, &library).await;
        services
            .collection
            .apply(
                &actor,
                SyncBatch {
                    device_id: "standalone-client".into(),
                    operations: vec![Operation {
                        id: Uuid::from_u128(705),
                        action: Action::PlaylistCreate {
                            playlist_id: Uuid::from_u128(706),
                            name: "Standalone owned playlist".into(),
                        },
                    }],
                },
            )
            .await
            .unwrap();
        let standalone_catalog = serde_json::to_value(
            services
                .catalog
                .recordings(&actor, Default::default())
                .await
                .unwrap(),
        )
        .unwrap();
        let standalone_events =
            serde_json::to_value(services.collection.events(&actor, 0).await.unwrap()).unwrap();
        Self {
            directory,
            original,
            original_bytes,
            standalone_catalog,
            standalone_events,
            services,
            actor,
        }
    }

    pub async fn bind(&self, remote: &Remote) -> Bound {
        let connection = self
            .services
            .replicas
            .connect(
                &self.actor,
                ConnectReplica {
                    endpoint: remote.endpoint.clone(),
                    credentials: Credentials {
                        username: "owned-peer".into(),
                        password: "owned-peer-password".into(),
                    },
                },
            )
            .await
            .unwrap();
        let bound = self
            .activate(connection.id, connection.client_id.as_str())
            .await;
        self.services
            .replicas
            .enqueue(
                &self.actor,
                &bound.session,
                Operation {
                    id: Uuid::from_u128(703),
                    action: Action::PlaylistCreate {
                        playlist_id: Uuid::from_u128(702),
                        name: "Owned replica playlist".into(),
                    },
                },
            )
            .await
            .unwrap();
        let outcome = self
            .services
            .replicas
            .synchronize(&self.actor, &bound.session)
            .await
            .unwrap();
        assert_eq!(
            (outcome.pushed, outcome.received, outcome.cursor),
            (1, 1, 1)
        );
        self.services
            .replicas
            .enqueue(&self.actor, &bound.session, pending())
            .await
            .unwrap();
        bound
    }

    pub async fn activate(&self, connection: Uuid, original_client_id: &str) -> Bound {
        let session = self
            .services
            .replicas
            .activate(&self.actor, connection)
            .await
            .unwrap();
        let saved = self
            .services
            .replicas
            .connections(&self.actor)
            .await
            .unwrap()
            .into_iter()
            .find(|item| item.id == connection)
            .unwrap();
        assert_eq!(saved.client_id.as_str(), original_client_id);
        Bound {
            connection,
            session,
            client_id: original_client_id.into(),
        }
    }
    pub async fn install(&self, bound: &Bound) -> hito_core::Result<CatalogSnapshot> {
        self.services
            .replicas
            .install_catalog(
                &self.actor,
                &bound.session,
                BeginCatalogSnapshot {
                    library_id: LIBRARY.into(),
                },
            )
            .await
    }
    pub async fn reopen(self) -> Self {
        self.services.replicas.close().await.unwrap();
        self.services.catalog_replication.close().await.unwrap();
        drop(self.services);
        let services = hito_core::open(config(self.directory.path()))
            .await
            .unwrap();
        let login = services.identity.login(credentials()).await.unwrap();
        let actor = services.identity.authenticate(&login.token).await.unwrap();
        Self {
            services,
            actor,
            ..self
        }
    }
    pub async fn verify_catalog(&self, bound: &Bound, recordings: usize) {
        self.verify_export(
            bound,
            Export {
                id: SNAPSHOT,
                recordings,
            },
        )
        .await;
    }
    pub async fn verify_export(&self, bound: &Bound, export: Export) {
        let started = Instant::now();
        let actual = self
            .services
            .replicas
            .catalog_snapshot(&self.actor, &bound.session, LIBRARY)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(actual, export.metadata());
        let mut identities = HashSet::with_capacity(export.recordings);
        let mut source_ids = HashSet::with_capacity(export.recordings);
        let mut releases = HashSet::with_capacity(export.recordings);
        for ordinal in 0..actual.page_count {
            let page = self
                .services
                .replicas
                .catalog_page(&self.actor, &bound.session, actual.id, ordinal)
                .await
                .unwrap();
            assert_eq!(
                serde_json::to_value(&page).unwrap(),
                serde_json::to_value(export.page(ordinal)).unwrap()
            );
            for fact in page.recordings {
                assert!(identities.insert(fact.id));
                for source in fact.sources {
                    assert!(source_ids.insert(source.id));
                    let appearance = source.release.unwrap();
                    assert!(releases.insert((appearance.id, appearance.entry_id)));
                }
            }
        }
        assert_eq!(
            (identities.len(), source_ids.len(), releases.len()),
            (export.recordings, export.recordings, export.recordings)
        );
        eprintln!(
            "catalog_storage_read recordings={} pages={} elapsed_ms={}",
            export.recordings,
            actual.page_count,
            started.elapsed().as_millis()
        );
    }
    pub async fn verify_owned_state(&self, bound: &Bound) {
        assert_eq!(std::fs::read(&self.original).unwrap(), self.original_bytes);
        assert_eq!(
            serde_json::to_value(
                self.services
                    .catalog
                    .recordings(&self.actor, Default::default())
                    .await
                    .unwrap()
            )
            .unwrap(),
            self.standalone_catalog
        );
        assert_eq!(
            serde_json::to_value(
                self.services
                    .collection
                    .events(&self.actor, 0)
                    .await
                    .unwrap()
            )
            .unwrap(),
            self.standalone_events
        );
        let standalone = self
            .services
            .collection
            .playlists(&self.actor)
            .await
            .unwrap();
        assert_eq!(standalone.len(), 1);
        assert_eq!(standalone[0].name, "Standalone owned playlist");
        let playlists = self
            .services
            .replicas
            .playlists(&self.actor, &bound.session, Browse::default())
            .await
            .unwrap();
        assert_eq!(playlists.total, 1);
        assert_eq!(playlists.items[0].name, "Pending owned rename");
        let changed = Operation {
            id: pending().id,
            action: Action::PlaylistRename {
                playlist_id: Uuid::from_u128(702),
                name: "Different body".into(),
            },
        };
        assert!(matches!(
            self.services
                .replicas
                .enqueue(&self.actor, &bound.session, changed)
                .await,
            Err(hito_core::Error::Conflict(_))
        ));
    }
    pub fn disk_usage(&self) -> DiskUsage {
        DiskUsage::read(self.directory.path())
    }
    pub async fn close(self) {
        self.services.replicas.close().await.unwrap();
        self.services.catalog_replication.close().await.unwrap();
    }
}

#[derive(Debug, Default)]
pub struct DiskUsage {
    pub database_bytes: u64,
    pub wal_bytes: u64,
    pub total_bytes: u64,
    pub allocated_bytes: u64,
}
impl DiskUsage {
    fn read(root: &Path) -> Self {
        use std::os::unix::fs::MetadataExt;
        let mut usage = Self::default();
        let mut remaining = vec![root.to_path_buf()];
        while let Some(directory) = remaining.pop() {
            for entry in std::fs::read_dir(directory).unwrap() {
                let entry = entry.unwrap();
                let metadata = entry.metadata().unwrap();
                if metadata.is_dir() {
                    remaining.push(entry.path());
                } else if metadata.is_file() {
                    usage.total_bytes += metadata.len();
                    usage.allocated_bytes += metadata.blocks() * 512;
                    let name = entry.file_name();
                    let name = name.to_str().unwrap();
                    if name.ends_with(".db") {
                        usage.database_bytes += metadata.len();
                    }
                    if name.ends_with(".db-wal") {
                        usage.wal_bytes += metadata.len();
                    }
                }
            }
        }
        usage
    }
}
