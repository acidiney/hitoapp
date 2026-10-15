use super::{PluginCredential, PluginRefresh, Plugins, RefreshPlugin, installations::installation};
use crate::{Actor, Error, Result};
use hito_plugin_sdk::Manifest;
use sha2::{Digest, Sha256};
use sqlx::Row;

impl Plugins {
    #[tracing::instrument(level = "info", skip_all, err(Debug), fields(account_id = actor.id()))]
    pub async fn refresh(
        &self,
        actor: &Actor,
        id: &str,
        request: RefreshPlugin,
    ) -> Result<PluginRefresh> {
        let row = sqlx::query("SELECT p.id,p.service_id,p.manifest,p.revision,p.checked_at,p.credential,p.credential IS NOT NULL AS credential_set,s.base_url,s.revision AS service_revision
            FROM plugin_installations p JOIN plugin_services s ON s.id = p.service_id AND s.active = 1
            WHERE p.id = ? AND p.user_id = ? AND p.active = 1")
            .bind(id).bind(actor.id()).fetch_optional(&self.store.pool).await?.ok_or(Error::NotFound)?;
        let base_url: String = row.try_get("base_url")?;
        let service_revision: i64 = row.try_get("service_revision")?;
        let saved: Option<Vec<u8>> = row.try_get("credential")?;
        let previous = installation(row)?;
        if previous.revision != request.revision {
            return Err(changed());
        }
        let (token, credential) = match request.credential {
            PluginCredential::Keep => (
                saved
                    .as_deref()
                    .map(|blob| {
                        self.vault
                            .reveal(actor.id(), id, &previous.service_id, blob)
                    })
                    .transpose()?,
                saved,
            ),
            PluginCredential::Replace { bearer_token } => {
                let token = zeroize::Zeroizing::new(bearer_token);
                let sealed = self
                    .vault
                    .seal(actor.id(), id, &previous.service_id, &token)?;
                (Some(token), Some(sealed))
            }
        };
        let manifest = self
            .remote
            .manifest(actor.id(), &base_url, token.as_deref().map(String::as_str))
            .await?;
        if manifest.id != previous.manifest.id {
            return Err(Error::Conflict(
                "The service returned a different plugin identity. Remove and reconnect to use it."
                    .into(),
            ));
        }
        let (encoded, digest) = fingerprint(&manifest)?;
        if request
            .accept_manifest_digest
            .as_ref()
            .is_some_and(|accepted| accepted != &digest)
        {
            return Err(Error::Conflict(
                "The plugin changed after review. Check the connection again.".into(),
            ));
        }
        let mut tx = self.store.pool.begin_with("BEGIN IMMEDIATE").await?;
        let current: i64 = sqlx::query_scalar(
            "SELECT p.revision FROM plugin_installations p
            JOIN plugin_services s ON s.id = p.service_id AND s.active = 1 AND s.revision = ?
            WHERE p.id = ? AND p.user_id = ? AND p.active = 1",
        )
        .bind(service_revision)
        .bind(id)
        .bind(actor.id())
        .fetch_optional(&mut *tx)
        .await?
        .ok_or(Error::NotFound)?;
        if current != previous.revision {
            return Err(changed());
        }
        if request.accept_manifest_digest.is_none() && digest != fingerprint(&previous.manifest)?.1
        {
            tx.commit().await?;
            tracing::info!(installation_id = id, "Plugin manifest review required");
            return Ok(PluginRefresh::ReviewRequired { manifest, digest });
        }
        let row = sqlx::query("UPDATE plugin_installations SET manifest = ?, credential = ?, revision = revision + 1, checked_at = unixepoch()
            WHERE id = ? AND user_id = ? AND active = 1 AND revision = ? RETURNING id,service_id,manifest,revision,checked_at,credential IS NOT NULL AS credential_set")
            .bind(encoded).bind(credential).bind(id).bind(actor.id()).bind(current).fetch_one(&mut *tx).await?;
        let installation = installation(row)?;
        tx.commit().await?;
        tracing::info!(
            installation_id = id,
            revision = installation.revision,
            "Plugin connection refreshed"
        );
        Ok(PluginRefresh::Updated { installation })
    }
}

fn changed() -> Error {
    Error::Conflict("This plugin connection changed. Reload Settings.".into())
}

fn fingerprint(manifest: &Manifest) -> Result<(String, String)> {
    let encoded = serde_json::to_string(manifest)
        .map_err(|_| Error::Internal("Plugin manifest could not be encoded".into()))?;
    let mut digest = Sha256::new();
    digest.update(b"hito.plugin.manifest.v1\0");
    digest.update(encoded.as_bytes());
    Ok((encoded, format!("{:x}", digest.finalize())))
}
