use crate::{Error, Result};
use hito_plugin_sdk::Manifest;
use sha2::{Digest, Sha256};

pub(super) fn fingerprint(manifest: &Manifest) -> Result<(String, String)> {
    let encoded = serde_json::to_string(manifest)
        .map_err(|_| Error::Internal("Plugin manifest could not be encoded".into()))?;
    let mut digest = Sha256::new();
    digest.update(b"hito.plugin.manifest.v1\0");
    digest.update(encoded.as_bytes());
    Ok((encoded, format!("{:x}", digest.finalize())))
}
