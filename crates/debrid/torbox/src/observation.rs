use crate::{Error, TorrentFileBinding, TorrentPhase};
use serde::Deserialize;

#[derive(Deserialize)]
struct Envelope {
    success: bool,
    error: Option<serde::de::IgnoredAny>,
    data: Option<Torrent>,
}

#[derive(Deserialize)]
struct Torrent {
    id: u64,
    hash: String,
    size: u64,
    download_finished: bool,
    download_present: bool,
    files: Vec<File>,
}

#[derive(Deserialize)]
struct File {
    id: u64,
    name: String,
    size: u64,
}

pub(crate) fn phase(bytes: &[u8], binding: &TorrentFileBinding) -> Result<TorrentPhase, Error> {
    let envelope: Envelope = serde_json::from_slice(bytes).map_err(|_| Error::InvalidResponse)?;
    if !envelope.success || envelope.error.is_some() {
        return Err(Error::ProviderRejected);
    }
    let torrent = envelope.data.ok_or(Error::InvalidResponse)?;
    if torrent.id != binding.torrent_id
        || torrent.hash != binding.info_hash
        || torrent.size != binding.torrent_bytes
    {
        return Err(Error::SourceChanged);
    }
    let mut matches = torrent
        .files
        .iter()
        .filter(|file| file.id == binding.file_id);
    let selected = matches.next().ok_or(Error::SourceChanged)?;
    if matches.next().is_some()
        || selected.name != binding.file_name
        || selected.size != binding.file_bytes
    {
        return Err(Error::SourceChanged);
    }
    Ok(if torrent.download_finished && torrent.download_present {
        TorrentPhase::ProviderCompleted
    } else {
        TorrentPhase::Incomplete
    })
}
