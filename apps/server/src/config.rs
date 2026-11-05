mod origins;
mod watching;

use hito_core::{
    CommunityConfig, CommunityRankingConfig, ConfiguredPluginService, CoreConfig, HostDeviceId,
    StorageConfig,
};
use origins::{parse_additional_origins, validate_combined_origins};
use sha2::{Digest, Sha256};
use std::{net::SocketAddr, path::PathBuf};
use watching::parse_folder_watching;

pub type RuntimeResult<T> = Result<T, Box<dyn std::error::Error + Send + Sync>>;

#[derive(Clone)]
pub struct Config {
    pub origin: String,
    pub additional_origins: Vec<String>,
    pub web_dir: PathBuf,
    pub asset_version: String,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            origin: "http://localhost:8080".into(),
            additional_origins: Vec::new(),
            web_dir: "apps/web/dist".into(),
            asset_version: "development".into(),
        }
    }
}

pub struct RuntimeConfig {
    pub core: CoreConfig,
    pub http: Config,
    pub listen: SocketAddr,
    pub json_logs: bool,
    pub plugin_services: Vec<ConfiguredPluginService>,
    pub(crate) community_config_invalid: bool,
    pub(crate) community_ranking_config_invalid: bool,
}

impl RuntimeConfig {
    pub async fn from_env() -> RuntimeResult<Self> {
        let additional_origins = match std::env::var("HITO_ADDITIONAL_ORIGINS") {
            Ok(value) => parse_additional_origins(&value)?,
            Err(std::env::VarError::NotPresent) => Vec::new(),
            Err(std::env::VarError::NotUnicode(_)) => {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidInput,
                    "HITO_ADDITIONAL_ORIGINS must contain valid UTF-8",
                )
                .into());
            }
        };
        let database = PathBuf::from(
            std::env::var("HITO_DATABASE").unwrap_or_else(|_| "/data/hito.db".into()),
        );
        let music_root =
            PathBuf::from(std::env::var("HITO_MUSIC_ROOT").unwrap_or_else(|_| "/music".into()));
        let web_dir =
            PathBuf::from(std::env::var("HITO_WEB_DIR").unwrap_or_else(|_| "/app/web".into()));
        let host_device = std::env::var("HITO_DEVICE_ID")
            .ok()
            .map(HostDeviceId::new)
            .transpose()?;
        let mut config = match std::env::var_os("HITO_STORAGE_MANIFEST") {
            Some(manifest) => {
                Self::load_with_manifest(database, manifest.into(), web_dir, host_device).await?
            }
            None => Self::load(database, music_root, web_dir, host_device).await?,
        };
        let community = match (
            std::env::var("HITO_COMMUNITY_ORIGIN"),
            std::env::var_os("HITO_COMMUNITY_ADMIN_TOKEN_FILE"),
        ) {
            (Err(std::env::VarError::NotPresent), None) => Ok(None),
            (Ok(origin), Some(file)) => CommunityConfig::new(origin, file.into()).map(Some),
            _ => Err(hito_core::Error::Invalid(
                "Community host configuration is incomplete".into(),
            )),
        };
        config.community_config_invalid = community.is_err();
        config.core.community = community.ok().flatten();
        let rankings = match std::env::var("HITO_COMMUNITY_RANKINGS_ORIGIN") {
            Err(std::env::VarError::NotPresent) => Ok(None),
            Ok(origin) => CommunityRankingConfig::new(origin).map(Some),
            Err(std::env::VarError::NotUnicode(_)) => Err(hito_core::Error::Invalid(
                "Community ranking host configuration is invalid".into(),
            )),
        };
        config.community_ranking_config_invalid = rankings.is_err();
        config.core.community_rankings = rankings.ok().flatten();
        if let Some(tool) = std::env::var_os("HITO_FFMPEG") {
            config.core.media_tools.ffmpeg = tool.into();
        }
        if let Some(tool) = std::env::var_os("HITO_FFPROBE") {
            config.core.media_tools.ffprobe = tool.into();
        }
        config.http.origin =
            std::env::var("HITO_PUBLIC_ORIGIN").unwrap_or_else(|_| "http://localhost:8080".into());
        validate_combined_origins(&config.http.origin, &additional_origins)?;
        config.http.additional_origins = additional_origins;
        config.core.folder_watching = parse_folder_watching(
            std::env::var("HITO_FOLDER_WATCH").ok().as_deref(),
            std::env::var("HITO_FOLDER_WATCH_POLL_SECONDS")
                .ok()
                .as_deref(),
        )?;
        config.core.metadata_contact = std::env::var("HITO_METADATA_CONTACT")
            .ok()
            .map(|contact| contact.trim().to_owned())
            .filter(|contact| !contact.is_empty());
        config.plugin_services = match std::env::var("HITO_PLUGIN_SERVICES") {
            Ok(value) => ConfiguredPluginService::parse_list(&value)?,
            Err(std::env::VarError::NotPresent) => Vec::new(),
            Err(std::env::VarError::NotUnicode(_)) => {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidInput,
                    "HITO_PLUGIN_SERVICES must contain valid UTF-8",
                )
                .into());
            }
        };
        config.listen = std::env::var("HITO_LISTEN")
            .unwrap_or_else(|_| "0.0.0.0:8080".into())
            .parse()?;
        config.json_logs = std::env::var("HITO_LOG_FORMAT").is_ok_and(|format| format == "json");
        Ok(config)
    }

    pub async fn load(
        database: PathBuf,
        music_root: PathBuf,
        web_dir: PathBuf,
        host_device: Option<HostDeviceId>,
    ) -> RuntimeResult<Self> {
        Self::load_storage(
            database,
            StorageConfig::LegacyFolder(music_root),
            web_dir,
            host_device,
        )
        .await
    }

    pub async fn load_with_manifest(
        database: PathBuf,
        manifest: PathBuf,
        web_dir: PathBuf,
        host_device: Option<HostDeviceId>,
    ) -> RuntimeResult<Self> {
        let directories = crate::storage_manifest::read(&manifest).await?;
        Self::load_storage(
            database,
            StorageConfig::SharedDirectories(directories),
            web_dir,
            host_device,
        )
        .await
    }

    async fn load_storage(
        database: PathBuf,
        storage: StorageConfig,
        web_dir: PathBuf,
        host_device: Option<HostDeviceId>,
    ) -> RuntimeResult<Self> {
        if let Some(parent) = database
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
        {
            tokio::fs::create_dir_all(parent).await?;
        }
        let asset_version = format!(
            "{:x}",
            Sha256::digest(tokio::fs::read(web_dir.join("assets/app.js")).await?)
        );
        Ok(Self {
            core: CoreConfig::with_storage(
                database.clone(),
                storage,
                crate::host_identity::load(&database, host_device).await?,
            ),
            http: Config {
                origin: Config::default().origin,
                additional_origins: Vec::new(),
                web_dir,
                asset_version,
            },
            listen: SocketAddr::from(([0, 0, 0, 0], 8080)),
            json_logs: false,
            plugin_services: Vec::new(),
            community_config_invalid: false,
            community_ranking_config_invalid: false,
        })
    }
}
