use futures_util::future::BoxFuture;
use hito_server::HostFolders;
use std::{
    ffi::OsString,
    path::{Path, PathBuf},
    sync::Arc,
};

pub trait FolderPicker: Send + Sync + 'static {
    fn pick(&self) -> BoxFuture<'_, Option<PathBuf>>;
}

pub struct MusicLookup {
    pub xdg_override: Option<OsString>,
    pub os_music: Option<PathBuf>,
    pub home: Option<PathBuf>,
}

pub fn music_folder(lookup: MusicLookup) -> Option<PathBuf> {
    lookup
        .xdg_override
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
        .or(lookup.os_music)
        .or_else(|| lookup.home.map(|home| home.join("Music")))
}

pub fn proposal(candidate: Option<PathBuf>) -> Option<PathBuf> {
    let path = candidate?;
    usable(&path).then_some(path)
}

fn usable(path: &Path) -> bool {
    path.is_dir() && std::fs::read_dir(path).is_ok()
}

struct Desktop {
    music: PathBuf,
    picker: Arc<dyn FolderPicker>,
}

pub(crate) fn host(music: PathBuf, picker: Arc<dyn FolderPicker>) -> Arc<dyn HostFolders> {
    Arc::new(Desktop { music, picker })
}

impl HostFolders for Desktop {
    fn suggestion(&self) -> BoxFuture<'_, Option<PathBuf>> {
        Box::pin(async move {
            let music = self.music.clone();
            tokio::task::spawn_blocking(move || proposal(Some(music)))
                .await
                .ok()
                .flatten()
        })
    }

    fn choose(&self) -> BoxFuture<'_, Option<PathBuf>> {
        self.picker.pick()
    }
}
