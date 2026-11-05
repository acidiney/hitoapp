use std::{
    ffi::OsString,
    path::{Path, PathBuf},
};

pub struct MediaToolSearch {
    path: Option<OsString>,
    well_known: Vec<PathBuf>,
    excluded: Vec<PathBuf>,
}

impl MediaToolSearch {
    pub fn new(path: Option<OsString>, well_known: Vec<PathBuf>, excluded: Vec<PathBuf>) -> Self {
        Self {
            path,
            well_known,
            excluded,
        }
    }

    pub fn system(excluded: Vec<PathBuf>) -> Self {
        Self::new(std::env::var_os("PATH"), well_known_directories(), excluded)
    }

    pub fn find(&self, name: &str) -> Option<PathBuf> {
        let on_path = self
            .path
            .iter()
            .flat_map(|path| std::env::split_paths(path))
            .filter(|directory| directory.is_absolute());
        on_path
            .chain(self.well_known.iter().cloned())
            .filter(|directory| !self.is_excluded(directory))
            .map(|directory| directory.join(executable_name(name)))
            .find(|candidate| is_executable_file(candidate))
    }

    fn is_excluded(&self, directory: &Path) -> bool {
        let directory = directory.canonicalize().unwrap_or(directory.to_owned());
        self.excluded.iter().any(|excluded| {
            let excluded = excluded.canonicalize().unwrap_or(excluded.clone());
            directory.starts_with(excluded)
        })
    }
}

pub(crate) fn resolve(tools: &mut hito_core::MediaTools, excluded: Vec<PathBuf>) {
    let search = MediaToolSearch::system(excluded);
    for (variable, name, slot) in [
        ("HITO_FFMPEG", "ffmpeg", &mut tools.ffmpeg),
        ("HITO_FFPROBE", "ffprobe", &mut tools.ffprobe),
    ] {
        if let Some(explicit) = std::env::var_os(variable) {
            *slot = explicit.into();
            continue;
        }
        match search.find(name) {
            Some(found) => {
                tracing::info!(
                    event = "media_tool_resolved",
                    tool = name,
                    "Media tool found"
                );
                *slot = found;
            }
            None => {
                tracing::warn!(
                    event = "media_tool_not_found",
                    tool = name,
                    "Media tool not found"
                );
            }
        }
    }
}

fn executable_name(name: &str) -> OsString {
    if cfg!(windows) {
        let mut file = OsString::from(name);
        file.push(".exe");
        file
    } else {
        OsString::from(name)
    }
}

#[cfg(unix)]
fn is_executable_file(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    std::fs::metadata(path)
        .map(|metadata| metadata.is_file() && metadata.permissions().mode() & 0o111 != 0)
        .unwrap_or(false)
}

#[cfg(not(unix))]
fn is_executable_file(path: &Path) -> bool {
    std::fs::metadata(path)
        .map(|metadata| metadata.is_file())
        .unwrap_or(false)
}

#[cfg(target_os = "macos")]
fn well_known_directories() -> Vec<PathBuf> {
    ["/opt/homebrew/bin", "/usr/local/bin", "/opt/local/bin"]
        .map(PathBuf::from)
        .into()
}

#[cfg(all(unix, not(target_os = "macos")))]
fn well_known_directories() -> Vec<PathBuf> {
    ["/usr/bin", "/usr/local/bin"].map(PathBuf::from).into()
}

#[cfg(windows)]
fn well_known_directories() -> Vec<PathBuf> {
    Vec::new()
}
