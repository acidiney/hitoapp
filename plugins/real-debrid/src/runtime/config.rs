use hito_debrid_real_debrid::{AudioKind, MediaAllowlist, RealDebridClient, TorrentFileBinding};
use hito_plugin_real_debrid::{Config, CuratedAudioRecording, InitError};
use hito_plugin_sdk::ProviderRecording;
use serde::Deserialize;
use std::{
    net::SocketAddr,
    path::{Path, PathBuf},
};
use zeroize::Zeroizing;

const MAX_TOKEN_FILE_BYTES: usize = 4098;
const MAX_CATALOG_FILE_BYTES: usize = 1_048_576;

pub(super) struct Settings {
    pub listen: SocketAddr,
    pub provider: Config,
}

impl Settings {
    pub(super) fn read() -> Result<Self, &'static str> {
        let mut listen = SocketAddr::from(([127, 0, 0, 1], 8096));
        let mut listen_seen = false;
        let mut provider_token_file: Option<PathBuf> = None;
        let mut service_token_file: Option<PathBuf> = None;
        let mut catalog_file: Option<PathBuf> = None;
        let mut arguments = std::env::args_os().skip(1);
        while let Some(argument) = arguments.next() {
            match argument.to_str() {
                Some("--listen") if !listen_seen => {
                    listen = arguments
                        .next()
                        .and_then(|value| value.into_string().ok())
                        .ok_or("missing_listen_address")?
                        .parse()
                        .map_err(|_| "invalid_listen_address")?;
                    listen_seen = true;
                }
                Some("--provider-token-file") if provider_token_file.is_none() => {
                    provider_token_file = Some(
                        arguments
                            .next()
                            .map(PathBuf::from)
                            .ok_or("missing_provider_token_file")?,
                    );
                }
                Some("--service-token-file") if service_token_file.is_none() => {
                    service_token_file = Some(
                        arguments
                            .next()
                            .map(PathBuf::from)
                            .ok_or("missing_service_token_file")?,
                    );
                }
                Some("--catalog-file") if catalog_file.is_none() => {
                    catalog_file = Some(
                        arguments
                            .next()
                            .map(PathBuf::from)
                            .ok_or("missing_catalog_file")?,
                    );
                }
                _ => return Err("invalid_arguments"),
            }
        }
        let mut provider_token =
            read_token(&provider_token_file.ok_or("missing_provider_token_file")?)?;
        let mut service_token =
            read_token(&service_token_file.ok_or("missing_service_token_file")?)?;
        if provider_token.as_str() == service_token.as_str() {
            return Err("credentials_must_differ");
        }
        let catalog = read_private_file(
            &catalog_file.ok_or("missing_catalog_file")?,
            MAX_CATALOG_FILE_BYTES,
        )?;
        let catalog: Catalog =
            serde_json::from_slice(&catalog).map_err(|_| "catalog_file_invalid")?;
        let media_origins =
            MediaAllowlist::new(catalog.media_origins).map_err(|_| "media_origins_invalid")?;
        let recordings: Vec<CuratedAudioRecording> = catalog
            .recordings
            .into_iter()
            .map(|entry| CuratedAudioRecording {
                recording: entry.recording,
                source_id: entry.source_id,
                label: entry.label,
                audio_kind: entry.audio_kind,
                binding: TorrentFileBinding {
                    torrent_id: entry.binding.torrent_id,
                    info_hash: entry.binding.info_hash,
                    file_id: entry.binding.file_id,
                    file_path: entry.binding.file_path,
                    file_bytes: entry.binding.file_bytes,
                },
            })
            .collect();
        let client = RealDebridClient::new(std::mem::take(&mut *provider_token))
            .map_err(|_| "provider_initialization_failed")?;
        let provider = Config::new(
            std::mem::take(&mut *service_token),
            client,
            media_origins,
            recordings,
        )
        .map_err(|error| match error {
            InitError::InvalidServiceCredential => "service_credential_invalid",
            InitError::InvalidCatalog => "catalog_file_invalid",
        })?;
        Ok(Self { listen, provider })
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Catalog {
    media_origins: Vec<String>,
    recordings: Vec<Recording>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Recording {
    recording: ProviderRecording,
    source_id: String,
    label: String,
    binding: Binding,
    #[serde(default, deserialize_with = "read_audio_kind")]
    audio_kind: AudioKind,
}

fn read_audio_kind<'de, D: serde::Deserializer<'de>>(input: D) -> Result<AudioKind, D::Error> {
    match String::deserialize(input)?.as_str() {
        "pcm16_wav" => Ok(AudioKind::Pcm16Wav),
        "mp3" => Ok(AudioKind::Mp3),
        _ => Err(serde::de::Error::custom("unsupported audio kind")),
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Binding {
    torrent_id: String,
    info_hash: String,
    file_id: u64,
    file_path: String,
    file_bytes: u64,
}

fn read_token(path: &Path) -> Result<Zeroizing<String>, &'static str> {
    let bytes = read_private_file(path, MAX_TOKEN_FILE_BYTES)?;
    let token = std::str::from_utf8(&bytes).map_err(|_| "credential_file_invalid")?;
    let token = token
        .strip_suffix("\r\n")
        .or_else(|| token.strip_suffix('\n'))
        .unwrap_or(token);
    Ok(Zeroizing::new(token.to_owned()))
}

#[cfg(unix)]
fn read_private_file(path: &Path, maximum: usize) -> Result<Zeroizing<Vec<u8>>, &'static str> {
    use std::{
        fs::OpenOptions,
        io::Read,
        os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt},
    };
    if !path.is_absolute() {
        return Err("operator_file_path_invalid");
    }
    let mut file = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
        .open(path)
        .map_err(|_| "operator_file_unavailable")?;
    let metadata = file.metadata().map_err(|_| "operator_file_unavailable")?;
    let private = |metadata: &std::fs::Metadata| {
        metadata.is_file()
            && metadata.nlink() == 1
            && metadata.uid() == unsafe { libc::geteuid() }
            && matches!(metadata.permissions().mode() & 0o7777, 0o400 | 0o600)
            && metadata.len() > 0
            && metadata.len() <= maximum as u64
    };
    if !private(&metadata) {
        return Err("operator_file_not_private");
    }
    let mut bytes = Zeroizing::new(Vec::with_capacity(metadata.len() as usize));
    (&mut file)
        .take(maximum as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| "operator_file_invalid")?;
    let after = file.metadata().map_err(|_| "operator_file_unavailable")?;
    if !private(&after)
        || bytes.len() as u64 != metadata.len()
        || after.len() != metadata.len()
        || after.mtime() != metadata.mtime()
        || after.mtime_nsec() != metadata.mtime_nsec()
        || after.ctime() != metadata.ctime()
        || after.ctime_nsec() != metadata.ctime_nsec()
    {
        return Err("operator_file_changed");
    }
    Ok(bytes)
}

#[cfg(not(unix))]
fn read_private_file(_path: &Path, _maximum: usize) -> Result<Zeroizing<Vec<u8>>, &'static str> {
    Err("operator_file_platform_unsupported")
}
