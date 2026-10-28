use super::{DirectoryRef, LibraryStorage};
use crate::{Actor, Error, Result, SharedDirectoryConfig};
use sha2::{Digest, Sha256};
use std::{
    path::{Path, PathBuf},
    time::Duration,
};

enum Located {
    Inside(DirectoryRef),
    Outside(PathBuf),
}

impl LibraryStorage {
    /// Reports whether trusted host code's folder already lies inside a grant.
    /// It never grants anything.
    #[tracing::instrument(level = "debug", skip_all, err(Debug), fields(account_id = actor.id()))]
    pub async fn find_host_directory(
        &self,
        actor: &Actor,
        path: Option<&Path>,
    ) -> Result<Option<DirectoryRef>> {
        actor.require_admin()?;
        let Some(path) = path else {
            return Ok(None);
        };
        Ok(match self.locate(path).await {
            Ok(Located::Inside(reference)) => Some(reference),
            _ => None,
        })
    }

    async fn locate(&self, path: &Path) -> Result<Located> {
        if !path.is_absolute() {
            return Err(unavailable());
        }
        let path = path.to_owned();
        let grants = self.directories.grants();
        tokio::time::timeout(
            Duration::from_secs(5),
            tokio::task::spawn_blocking(move || locate_blocking(&path, &grants.list)),
        )
        .await
        .map_err(|_| unavailable())?
        .map_err(|_| unavailable())?
    }

    /// Maps a folder chosen by trusted host code to a shared-directory reference,
    /// granting it when no existing grant contains it. Web input never reaches this.
    #[tracing::instrument(level = "info", skip_all, err(Debug), fields(account_id = actor.id()))]
    pub async fn grant_host_directory(&self, actor: &Actor, path: &Path) -> Result<DirectoryRef> {
        actor.require_admin()?;
        let located = self.locate(path).await?;
        let (reference, granted) = match located {
            Located::Inside(reference) => (reference, false),
            Located::Outside(root) => {
                let id = format!("host-{}", &digest(&root)[..16]);
                let name = label(&root);
                self.directories.add(SharedDirectoryConfig {
                    id: id.clone(),
                    name,
                    root,
                })?;
                (
                    DirectoryRef {
                        directory_id: id,
                        components: Vec::new(),
                    },
                    true,
                )
            }
        };
        tracing::info!(
            event = "host_directory_located",
            granted,
            "Host folder mapped to a shared directory"
        );
        Ok(reference)
    }
}

fn locate_blocking(path: &Path, grants: &[SharedDirectoryConfig]) -> Result<Located> {
    let canonical = path.canonicalize().map_err(|_| unavailable())?;
    std::fs::read_dir(&canonical).map_err(|_| unavailable())?;
    for grant in grants {
        let Ok(root) = grant.root.canonicalize() else {
            continue;
        };
        if let Ok(relative) = canonical.strip_prefix(&root) {
            let components = relative
                .components()
                .map(|part| part.as_os_str().to_str().map(str::to_owned))
                .collect::<Option<Vec<_>>>()
                .ok_or_else(unavailable)?;
            return Ok(Located::Inside(DirectoryRef {
                directory_id: grant.id.clone(),
                components,
            }));
        }
    }
    Ok(Located::Outside(canonical))
}

fn digest(root: &Path) -> String {
    Sha256::digest(root.as_os_str().as_encoded_bytes())
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn label(root: &Path) -> String {
    root.file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .filter(|name| !name.trim().is_empty() && name.len() <= 256)
        .filter(|name| !name.chars().any(char::is_control))
        .unwrap_or_else(|| "Music folder".into())
}

fn unavailable() -> Error {
    Error::Invalid("Choose a folder that exists and can be read".into())
}
