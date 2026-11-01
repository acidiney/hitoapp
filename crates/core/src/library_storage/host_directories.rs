use super::{DirectoryRef, LibraryStorage};
use crate::{Actor, Error, Result, SharedDirectoryConfig};
use sha2::{Digest, Sha256};
use sqlx::Row;
use std::{
    path::{Path, PathBuf},
    time::Duration,
};

enum Located {
    Inside(DirectoryRef, PathBuf),
    Outside(PathBuf),
}

pub(super) struct Guard {
    pub(super) data_dir: Option<PathBuf>,
    pub(super) home: Option<PathBuf>,
}

impl Guard {
    pub(super) async fn load(data_dir: &Path) -> Self {
        let home = std::env::var_os("HOME")
            .or_else(|| std::env::var_os("USERPROFILE"))
            .map(PathBuf::from);
        Self {
            data_dir: tokio::fs::canonicalize(data_dir).await.ok(),
            home: match home {
                Some(home) => tokio::fs::canonicalize(home).await.ok(),
                None => None,
            },
        }
    }

    pub(super) fn denies(&self, candidate: &Path) -> bool {
        candidate.parent().is_none()
            || self.home.as_deref() == Some(candidate)
            || self
                .data_dir
                .as_deref()
                .is_some_and(|data| data.starts_with(candidate))
    }
}

impl LibraryStorage {
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
            Ok(Located::Inside(reference, _)) => Some(reference),
            _ => None,
        })
    }

    async fn locate(&self, path: &Path) -> Result<Located> {
        if !path.is_absolute() {
            return Err(unavailable());
        }
        let path = path.to_owned();
        let grants = self.directories.grants();
        let guard = Guard::load(&self.data_dir).await;
        tokio::time::timeout(
            Duration::from_secs(5),
            tokio::task::spawn_blocking(move || locate_blocking(&path, &grants.list, &guard)),
        )
        .await
        .map_err(|_| unavailable())?
        .map_err(|_| unavailable())?
    }

    #[tracing::instrument(level = "info", skip_all, err(Debug), fields(account_id = actor.id()))]
    pub async fn grant_host_directory(&self, actor: &Actor, path: &Path) -> Result<DirectoryRef> {
        actor.require_admin()?;
        let located = self.locate(path).await?;
        let (reference, granted) = match &located {
            Located::Inside(reference, canonical) => {
                self.refuse_crossing(canonical, &reference.directory_id)
                    .await?;
                (reference.clone(), false)
            }
            Located::Outside(root) => {
                let grant = host_grant(root.clone())?;
                self.refuse_crossing(root, &grant.id).await?;
                let id = grant.id.clone();
                self.directories.add(grant)?;
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

    async fn refuse_crossing(&self, candidate: &Path, grant: &str) -> Result<()> {
        let rows = sqlx::query(
            "SELECT directory_id, root_path, relative_path FROM music_folders WHERE host_device = ? AND connected = 1",
        )
        .bind(self.host.as_str())
        .fetch_all(&self.store.pool)
        .await?;
        let mut connected = Vec::new();
        for row in rows {
            let root: String = row.get("root_path");
            let relative: String = row.get("relative_path");
            let directory: Option<String> = row.get("directory_id");
            if let Ok(root) = tokio::fs::canonicalize(root).await {
                connected.push((directory, root.join(relative)));
            }
        }
        let crossing = connected.iter().any(|(directory, folder)| {
            (folder != candidate && folder.starts_with(candidate))
                || (candidate.starts_with(folder) && directory.as_deref() != Some(grant))
        });
        if crossing {
            return Err(Error::Conflict(
                "This folder overlaps a music folder that is already connected".into(),
            ));
        }
        Ok(())
    }
}

fn locate_blocking(
    path: &Path,
    grants: &[SharedDirectoryConfig],
    guard: &Guard,
) -> Result<Located> {
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
            return Ok(Located::Inside(
                DirectoryRef {
                    directory_id: grant.id.clone(),
                    components,
                },
                canonical,
            ));
        }
    }
    if guard.denies(&canonical) {
        return Err(Error::Conflict(
            "This folder holds Hito's private data or is a system or home folder. Choose a music folder inside it instead".into(),
        ));
    }
    Ok(Located::Outside(canonical))
}

pub(super) fn host_grant(root: PathBuf) -> Result<SharedDirectoryConfig> {
    let text = root.to_str().ok_or_else(|| {
        Error::Invalid("Choose a folder whose path uses standard text characters".into())
    })?;
    Ok(SharedDirectoryConfig {
        id: format!("host-{}", &digest(text)[..16]),
        name: label(&root),
        root,
    })
}

fn digest(root: &str) -> String {
    Sha256::digest(root.as_bytes())
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

#[cfg(test)]
mod tests {
    use super::Guard;
    use std::path::{Path, PathBuf};

    fn guard() -> Guard {
        Guard {
            data_dir: Some(PathBuf::from("/home/a/.local/share/hito")),
            home: Some(PathBuf::from("/home/a")),
        }
    }

    #[test]
    fn roots_home_and_data_ancestors_are_denied() {
        let guard = guard();
        for denied in [
            "/",
            "/home/a",
            "/home/a/.local/share/hito",
            "/home/a/.local",
        ] {
            assert!(guard.denies(Path::new(denied)), "{denied}");
        }
        for allowed in [
            "/home/a/Music",
            "/home/a/.local/share/hito/sub",
            "/mnt/music",
        ] {
            assert!(!guard.denies(Path::new(allowed)), "{allowed}");
        }
    }
}
