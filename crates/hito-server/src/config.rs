use hito_core::{CoreConfig, HostDeviceId, StorageConfig};
use sha2::{Digest, Sha256};
use std::{net::SocketAddr, path::PathBuf};

pub type RuntimeResult<T> = Result<T, Box<dyn std::error::Error + Send + Sync>>;

#[derive(Clone)]
pub struct Config {
    pub origin: String,
    pub web_dir: PathBuf,
    pub asset_version: String,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            origin: "http://localhost:8080".into(),
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
}

impl RuntimeConfig {
    pub async fn from_env() -> RuntimeResult<Self> {
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
        if let Some(tool) = std::env::var_os("HITO_FFMPEG") {
            config.core.media_tools.ffmpeg = tool.into();
        }
        if let Some(tool) = std::env::var_os("HITO_FFPROBE") {
            config.core.media_tools.ffprobe = tool.into();
        }
        config.http.origin =
            std::env::var("HITO_PUBLIC_ORIGIN").unwrap_or_else(|_| "http://localhost:8080".into());
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
            core: CoreConfig {
                database: database.clone(),
                storage,
                host_device: crate::host_identity::load(&database, host_device).await?,
                media_tools: Default::default(),
                podcast_network: Default::default(),
            },
            http: Config {
                origin: Config::default().origin,
                web_dir,
                asset_version,
            },
            listen: SocketAddr::from(([0, 0, 0, 0], 8080)),
            json_logs: false,
        })
    }
}
