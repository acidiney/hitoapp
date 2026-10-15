use super::{LibraryStorage, root::Root};
use crate::{Actor, Error, Result, SharedDirectoryConfig, StorageConfig};
use serde::Serialize;
use std::{collections::BTreeSet, path::PathBuf, sync::Arc};
use tokio::sync::Semaphore;
use utoipa::ToSchema;

#[derive(Clone)]
pub(crate) struct Directories {
    pub(super) grants: Arc<[SharedDirectoryConfig]>,
    pub(super) legacy: Option<PathBuf>,
    slots: Arc<Semaphore>,
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
        Ok(Self {
            grants: grants.into(),
            legacy,
            slots: Arc::new(Semaphore::new(4)),
        })
    }

    pub(super) async fn open(&self, id: &str) -> Result<Root> {
        let grant = self
            .grants
            .iter()
            .find(|grant| grant.id == id)
            .ok_or(Error::NotFound)?;
        Root::open(&grant.root, &self.slots).await
    }
}

impl LibraryStorage {
    #[tracing::instrument(level = "debug", skip_all, err(Debug), fields(account_id = actor.id()))]
    pub async fn shared_directories(&self, actor: &Actor) -> Result<Vec<SharedDirectory>> {
        actor.require_admin()?;
        let mut result = Vec::new();
        for grant in self.directories.grants.iter() {
            result.push(SharedDirectory {
                id: grant.id.clone(),
                name: grant.name.clone(),
                available: self.directories.open(&grant.id).await.is_ok(),
            });
        }
        tracing::debug!(
            event = "shared_directories_listed",
            count = result.len(),
            "Shared directory status checked"
        );
        Ok(result)
    }
}
