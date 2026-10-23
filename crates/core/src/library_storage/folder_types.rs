use super::Job;
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

#[derive(Clone, Deserialize, Serialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct DirectoryRef {
    pub directory_id: String,
    #[serde(default)]
    pub components: Vec<String>,
}

#[derive(Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct ConnectFolder {
    pub directory: DirectoryRef,
    pub name: Option<String>,
}

#[derive(Serialize, ToSchema)]
pub struct MusicFolder {
    pub id: String,
    pub library_id: String,
    pub host_device: String,
    pub directory: Option<DirectoryRef>,
    pub name: String,
    pub revision: i64,
    pub connected: bool,
    pub available: bool,
    pub last_scan_at: Option<i64>,
    pub latest_job: Option<Job>,
}

#[derive(Clone, sqlx::FromRow)]
pub(super) struct Folder {
    pub id: String,
    pub library_id: String,
    pub host_device: String,
    pub directory_id: Option<String>,
    pub relative_path: String,
    pub root_path: String,
    pub name: String,
    pub connected: bool,
    pub bound: bool,
    pub revision: i64,
    pub generation: i64,
    pub last_scan_at: Option<i64>,
}

impl Folder {
    pub fn directory(&self) -> Option<DirectoryRef> {
        self.directory_id.as_ref().map(|id| DirectoryRef {
            directory_id: id.clone(),
            components: self
                .relative_path
                .split('/')
                .filter(|part| !part.is_empty())
                .map(str::to_owned)
                .collect(),
        })
    }
}
