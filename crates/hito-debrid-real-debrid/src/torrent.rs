use crate::Error;
use serde::Deserialize;
use std::collections::HashSet;
use zeroize::Zeroizing;

#[derive(Clone, PartialEq, Eq)]
pub struct TorrentFileBinding {
    pub torrent_id: String,
    pub info_hash: String,
    pub file_id: u64,
    pub file_path: String,
    pub file_bytes: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TorrentPhase {
    Downloading,
    ProviderCompleted,
}

pub struct TorrentObservation {
    pub binding: TorrentFileBinding,
    pub phase: TorrentPhase,
    pub progress_basis_points: Option<u16>,
}

impl TorrentFileBinding {
    pub(crate) fn validate(&self) -> Result<(), Error> {
        if !bounded_text(&self.torrent_id, 256)
            || matches!(self.torrent_id.as_str(), "." | "..")
            || self.info_hash.len() != 40
            || !self.info_hash.bytes().all(|byte| byte.is_ascii_hexdigit())
            || self.file_id == 0
            || !bounded_text(&self.file_path, 4096)
            || !self.file_path.starts_with('/')
            || self.file_bytes == 0
        {
            return Err(Error::InvalidBinding);
        }
        Ok(())
    }
}

#[derive(Deserialize)]
struct Info {
    id: String,
    hash: String,
    bytes: u64,
    progress: u16,
    status: String,
    files: Vec<File>,
    links: Vec<String>,
}

#[derive(Deserialize)]
struct File {
    id: u64,
    path: String,
    bytes: u64,
    selected: u8,
}

pub(crate) fn observation(
    bytes: &[u8],
    binding: &TorrentFileBinding,
) -> Result<TorrentObservation, Error> {
    let info: Info = serde_json::from_slice(bytes).map_err(|_| Error::InvalidResponse)?;
    if info.id != binding.torrent_id || info.hash != binding.info_hash {
        return Err(Error::SourceChanged);
    }
    if info.progress > 100 || info.files.len() > 512 {
        return Err(Error::InvalidResponse);
    }
    let mut ids = HashSet::new();
    for file in &info.files {
        if !ids.insert(file.id) || file.selected > 1 {
            return Err(Error::InvalidResponse);
        }
    }
    let file = info
        .files
        .iter()
        .find(|file| file.id == binding.file_id)
        .ok_or(Error::SourceChanged)?;
    if file.path != binding.file_path
        || file.bytes != binding.file_bytes
        || file.selected != 1
        || info.bytes != binding.file_bytes
    {
        return Err(Error::SourceChanged);
    }
    if info.files.iter().filter(|file| file.selected == 1).count() != 1 {
        return Err(Error::InvalidResponse);
    }
    let phase = match info.status.as_str() {
        "downloading" if info.links.is_empty() => TorrentPhase::Downloading,
        "downloaded" if info.progress == 100 && info.links.len() == 1 => {
            let link = Zeroizing::new(
                info.links
                    .into_iter()
                    .next()
                    .ok_or(Error::InvalidResponse)?,
            );
            validate_link(&link)?;
            TorrentPhase::ProviderCompleted
        }
        "downloading" | "downloaded" => return Err(Error::InvalidResponse),
        _ => return Err(Error::UnsupportedState),
    };
    Ok(TorrentObservation {
        binding: binding.clone(),
        phase,
        progress_basis_points: Some(info.progress * 100),
    })
}

fn bounded_text(value: &str, maximum: usize) -> bool {
    !value.trim().is_empty() && value.len() <= maximum && !value.chars().any(char::is_control)
}

fn validate_link(value: &str) -> Result<(), Error> {
    if !bounded_text(value, 8192) || value.chars().any(char::is_whitespace) || value.contains('\\')
    {
        return Err(Error::InvalidResponse);
    }
    let link = reqwest::Url::parse(value).map_err(|_| Error::InvalidResponse)?;
    if link.scheme() != "https"
        || link.host_str().is_none()
        || !link.username().is_empty()
        || link.password().is_some()
        || link.fragment().is_some()
    {
        return Err(Error::InvalidResponse);
    }
    Ok(())
}
