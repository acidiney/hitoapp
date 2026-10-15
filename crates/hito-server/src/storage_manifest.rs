use crate::config::RuntimeResult;
use hito_core::SharedDirectoryConfig;
use serde::Deserialize;
use std::path::Path;
use tokio::io::AsyncReadExt;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Manifest {
    version: u32,
    directories: Vec<SharedDirectoryConfig>,
}

pub(crate) async fn read(path: &Path) -> RuntimeResult<Vec<SharedDirectoryConfig>> {
    let mut bytes = Vec::new();
    tokio::fs::File::open(path)
        .await
        .map_err(|_| "Installation storage configuration could not be opened")?
        .take(1_048_577)
        .read_to_end(&mut bytes)
        .await
        .map_err(|_| "Installation storage configuration could not be read")?;
    if bytes.len() > 1_048_576 {
        return Err("Installation storage configuration is too large".into());
    }
    let manifest: Manifest = serde_json::from_slice(&bytes)
        .map_err(|_| "Installation storage configuration is invalid")?;
    if manifest.version != 1 || manifest.directories.len() > 64 {
        return Err("Unsupported installation storage configuration".into());
    }
    tracing::info!(
        event = "storage_manifest_loaded",
        count = manifest.directories.len(),
        "Installation directory grants loaded"
    );
    Ok(manifest.directories)
}
