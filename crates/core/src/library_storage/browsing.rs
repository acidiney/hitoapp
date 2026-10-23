use super::{DirectoryRef, LibraryStorage, root::Root};
use crate::{Actor, Error, Result};
use serde::{Deserialize, Serialize};
use std::{
    sync::atomic::{AtomicBool, Ordering},
    time::{Duration, Instant},
};
use utoipa::ToSchema;

#[derive(Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct BrowseDirectories {
    pub directory: DirectoryRef,
    pub offset: Option<u32>,
}

#[derive(Serialize, ToSchema)]
pub struct DirectoryEntry {
    pub name: String,
    pub directory: DirectoryRef,
}

#[derive(Serialize, ToSchema)]
pub struct DirectoryPage {
    pub directory: DirectoryRef,
    pub directories: Vec<DirectoryEntry>,
    pub next_offset: Option<u32>,
}

impl LibraryStorage {
    #[tracing::instrument(level = "debug", skip_all, err(Debug), fields(account_id = actor.id()))]
    pub async fn browse(&self, actor: &Actor, request: BrowseDirectories) -> Result<DirectoryPage> {
        actor.require_admin()?;
        if request.offset.unwrap_or(0) > 1_000_000 {
            return Err(Error::Invalid(
                "Refresh the folder browser to continue".into(),
            ));
        }
        let operation = async {
            let root = self.directories.open_selected(&request.directory).await?;
            super::blocking::run(root.slots.clone(), move |cancelled| {
                read(root, request, cancelled)
            })
            .await
        };
        let page = tokio::time::timeout(Duration::from_secs(5), operation)
            .await
            .map_err(|_| stopped())??;
        tracing::debug!(
            event = "shared_directory_browsed",
            count = page.directories.len(),
            "Shared directory page read"
        );
        Ok(page)
    }
}

fn read(root: Root, request: BrowseDirectories, cancelled: &AtomicBool) -> Result<DirectoryPage> {
    let started = Instant::now();
    let check = || {
        if cancelled.load(Ordering::Relaxed) || started.elapsed() >= Duration::from_secs(4) {
            Err(stopped())
        } else {
            Ok(())
        }
    };
    let mut entries = root.directory.entries().map_err(|_| stopped())?;
    let offset = request.offset.unwrap_or(0);
    for _ in 0..offset {
        check()?;
        if entries.next().transpose().map_err(|_| stopped())?.is_none() {
            break;
        }
    }
    let mut directories = Vec::new();
    let mut read = 0;
    while read < 512 && directories.len() < 64 {
        check()?;
        let Some(entry) = entries.next() else {
            break;
        };
        read += 1;
        let entry = entry.map_err(|_| stopped())?;
        if !entry.file_type().map_err(|_| stopped())?.is_dir() {
            continue;
        }
        let name = entry.file_name();
        let Some(name) = name.to_str() else {
            continue;
        };
        let mut directory = request.directory.clone();
        directory.components.push(name.to_owned());
        if super::directories::validate(&directory).is_err() {
            continue;
        }
        directories.push(DirectoryEntry {
            name: name.to_owned(),
            directory,
        });
    }
    check()?;
    let more = entries.next().transpose().map_err(|_| stopped())?.is_some();
    directories.sort_unstable_by(|a, b| a.name.cmp(&b.name));
    Ok(DirectoryPage {
        directory: request.directory,
        directories,
        next_offset: more.then_some(offset + read),
    })
}

fn stopped() -> Error {
    Error::Internal("Shared directory could not be read. Refresh and try again".into())
}
