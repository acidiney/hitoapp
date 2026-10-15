use super::{DirectoryRef, LibraryStorage, root::Root};
use crate::{Actor, Error, Result, SharedDirectoryConfig, StorageConfig};
use serde::Serialize;
use std::{
    collections::{BTreeMap, BTreeSet},
    path::PathBuf,
    sync::Arc,
};
use tokio::sync::Semaphore;
use utoipa::ToSchema;

#[derive(Clone)]
pub(crate) struct Directories {
    pub(super) grants: Arc<[SharedDirectoryConfig]>,
    pub(super) legacy: Option<PathBuf>,
    slots: Arc<BTreeMap<String, GrantSlots>>,
}

struct GrantSlots {
    operations: Arc<Semaphore>,
    walks: Arc<Semaphore>,
}

#[derive(Serialize, ToSchema)]
pub struct SharedDirectory {
    pub id: String,
    pub name: String,
    pub available: bool,
}

impl Directories {
    pub(crate) fn new(config: StorageConfig) -> Result<Self> {
        let (grants, legacy) = match config {
            StorageConfig::LegacyFolder(root) => (
                vec![SharedDirectoryConfig {
                    id: "legacy-music".into(),
                    name: "Music folder".into(),
                    root: root.clone(),
                }],
                Some(root),
            ),
            StorageConfig::SharedDirectories(grants) => (grants, None),
        };
        if grants.len() > 64 {
            return Err(Error::Invalid("Share at most 64 music directories".into()));
        }
        let mut ids = BTreeSet::new();
        for grant in &grants {
            if grant.id.is_empty()
                || grant.id.len() > 128
                || !grant
                    .id
                    .bytes()
                    .all(|v| v.is_ascii_alphanumeric() || b"-_".contains(&v))
                || !ids.insert(&grant.id)
            {
                return Err(Error::Invalid(
                    "Shared directory IDs must be unique opaque identifiers".into(),
                ));
            }
            if grant.name.trim().is_empty()
                || grant.name.len() > 256
                || grant.name.chars().any(char::is_control)
            {
                return Err(Error::Invalid(
                    "Use a shared directory name of 1 to 256 bytes".into(),
                ));
            }
        }
        let slots = grants
            .iter()
            .map(|grant| {
                (
                    grant.id.clone(),
                    GrantSlots {
                        operations: Arc::new(Semaphore::new(1)),
                        walks: Arc::new(Semaphore::new(1)),
                    },
                )
            })
            .collect();
        Ok(Self {
            grants: grants.into(),
            legacy,
            slots: Arc::new(slots),
        })
    }

    pub(super) async fn open(&self, id: &str) -> Result<Root> {
        self.open_selected(&DirectoryRef {
            directory_id: id.into(),
            components: Vec::new(),
        })
        .await
    }

    pub(crate) async fn open_selected(&self, selection: &DirectoryRef) -> Result<Root> {
        validate(selection)?;
        let grant = self
            .grants
            .iter()
            .find(|grant| grant.id == selection.directory_id)
            .ok_or(Error::NotFound)?;
        let slots = self.slots.get(&grant.id).ok_or(Error::NotFound)?;
        Root::open(
            &grant.root,
            &slots.operations,
            &slots.walks,
            &selection.components,
        )
        .await
    }

    pub(crate) async fn open_media(
        &self,
        selection: &DirectoryRef,
        relative: PathBuf,
    ) -> Result<OpenedMedia> {
        let root = self
            .open_selected(selection)
            .await
            .map_err(|_| Error::NotFound)?;
        super::blocking::run(root.slots.clone(), move |cancelled| {
            let path = root.path().join(relative);
            let file = root.open_file(&path).map_err(|_| Error::NotFound)?;
            super::blocking::check(cancelled)?;
            Ok(OpenedMedia { root, path, file })
        })
        .await
    }
}

pub(crate) struct OpenedMedia {
    pub root: Root,
    pub path: PathBuf,
    pub file: std::fs::File,
}

pub(super) fn validate(selection: &DirectoryRef) -> Result<()> {
    if selection.components.len() > 64
        || selection.components.iter().any(|part| {
            part.is_empty()
                || part.len() > 255
                || part == "."
                || part == ".."
                || part.contains(['/', '\\', '\0'])
                || part.contains(':')
        })
    {
        return Err(Error::Invalid(
            "Choose a directory inside a shared location".into(),
        ));
    }
    Ok(())
}

impl LibraryStorage {
    #[tracing::instrument(level = "debug", skip_all, err(Debug), fields(account_id = actor.id()))]
    pub async fn shared_directories(&self, actor: &Actor) -> Result<Vec<SharedDirectory>> {
        actor.require_admin()?;
        let mut pending = tokio::task::JoinSet::new();
        for (index, grant) in self.directories.grants.iter().cloned().enumerate() {
            let directories = self.directories.clone();
            pending.spawn(async move {
                (
                    index,
                    SharedDirectory {
                        available: directories.open(&grant.id).await.is_ok(),
                        id: grant.id,
                        name: grant.name,
                    },
                )
            });
        }
        let mut result = Vec::new();
        while let Some(directory) = pending.join_next().await {
            result.push(
                directory.map_err(|_| Error::Internal("Directory status check stopped".into()))?,
            );
        }
        result.sort_unstable_by_key(|(index, _)| *index);
        let result: Vec<_> = result.into_iter().map(|(_, directory)| directory).collect();
        tracing::debug!(
            event = "shared_directories_listed",
            count = result.len(),
            "Shared directory status checked"
        );
        Ok(result)
    }
}
