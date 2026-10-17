use super::Plugins;
use crate::{AudioRequest, Error, OpenedAudio, Result};
use sha2::{Digest, Sha256};
use sqlx::{Row, SqliteConnection};

#[derive(Clone, sqlx::FromRow)]
pub(crate) struct DownloadOrigin {
    pub provider_source_id: String,
    pub binding_id: String,
    pub binding_revision: i64,
    pub remote_source_id: String,
    pub installation_id: String,
    pub installation_revision: i64,
    pub service_id: String,
    pub service_revision: i64,
    pub credential_digest: String,
    pub audio_generation: i64,
}

impl Plugins {
    pub(crate) async fn capture_download_origin_on(
        &self,
        connection: &mut SqliteConnection,
        account: &str,
        recording: &str,
    ) -> Result<DownloadOrigin> {
        let row = sqlx::query("SELECT s.id AS provider_source_id,item.id AS binding_id,item.revision AS binding_revision,
            ps.provider_source_id AS remote_source_id,p.id AS installation_id,p.revision AS installation_revision,
            p.service_id,service.revision AS service_revision,p.credential,p.audio_generation
            FROM sources s JOIN provider_sources ps ON ps.source_id=s.id AND ps.recording_id=s.recording_id
            JOIN provider_items item ON item.id=ps.binding_id AND item.retired=0
            JOIN provider_source_state state ON state.binding_id=item.id AND state.generation=ps.generation
            JOIN plugin_installations p ON p.id=ps.installation_id AND p.user_id=? AND p.active=1
            JOIN plugin_services service ON service.id=p.service_id AND service.active=1
            JOIN source_choice_bindings permitted ON permitted.binding_id=item.id AND permitted.recording_id=s.recording_id AND permitted.user_id=p.user_id
            WHERE s.recording_id=? AND s.ready=1
            AND EXISTS(SELECT 1 FROM json_each(p.manifest,'$.capabilities') WHERE value='sources')
            ORDER BY s.id LIMIT 1")
            .bind(account).bind(recording).fetch_optional(connection).await?
            .ok_or_else(|| Error::Conflict("No verified provider audio source is available for this download".into()))?;
        let credential: Option<Vec<u8>> = row.try_get("credential")?;
        Ok(DownloadOrigin {
            provider_source_id: row.try_get("provider_source_id")?,
            binding_id: row.try_get("binding_id")?,
            binding_revision: row.try_get("binding_revision")?,
            remote_source_id: row.try_get("remote_source_id")?,
            installation_id: row.try_get("installation_id")?,
            installation_revision: row.try_get("installation_revision")?,
            service_id: row.try_get("service_id")?,
            service_revision: row.try_get("service_revision")?,
            credential_digest: credential_digest(credential.as_deref()),
            audio_generation: row.try_get("audio_generation")?,
        })
    }

    #[tracing::instrument(level = "debug", skip_all, err(Debug), fields(account_id = account))]
    pub(crate) async fn open_download(
        &self,
        account: &str,
        origin: &DownloadOrigin,
    ) -> Result<OpenedAudio> {
        let row = sqlx::query("SELECT p.credential,service.base_url FROM plugin_installations p
            JOIN plugin_services service ON service.id=p.service_id AND service.active=1 AND service.revision=?
            WHERE p.id=? AND p.user_id=? AND p.active=1 AND p.revision=? AND p.service_id=? AND p.audio_generation=?
            AND EXISTS(SELECT 1 FROM json_each(p.manifest,'$.capabilities') WHERE value='sources')")
            .bind(origin.service_revision).bind(&origin.installation_id).bind(account)
            .bind(origin.installation_revision).bind(&origin.service_id).bind(origin.audio_generation)
            .fetch_optional(&self.store.pool).await?.ok_or(Error::NotFound)?;
        let credential: Option<Vec<u8>> = row.try_get("credential")?;
        if credential_digest(credential.as_deref()) != origin.credential_digest {
            return Err(Error::NotFound);
        }
        let token = credential
            .as_deref()
            .map(|blob| {
                self.vault
                    .reveal(account, &origin.installation_id, &origin.service_id, blob)
            })
            .transpose()?;
        let authority = super::AudioAuthority {
            store: self.store.clone(),
            account: account.into(),
            installation: origin.installation_id.clone(),
            service: origin.service_id.clone(),
            credential,
            source: origin.provider_source_id.clone(),
            remote_source: origin.remote_source_id.clone(),
            generation: origin.audio_generation,
            binding: origin.binding_id.clone(),
            binding_revision: origin.binding_revision,
        };
        authority.check().await?;
        self.media
            .open(
                row.try_get("base_url")?,
                token.as_deref().map(String::as_str),
                AudioRequest::default(),
                authority,
            )
            .await
    }
}

fn credential_digest(credential: Option<&[u8]>) -> String {
    let mut digest = Sha256::new();
    digest.update(if credential.is_some() {
        b"some".as_slice()
    } else {
        b"none".as_slice()
    });
    if let Some(credential) = credential {
        digest.update(credential);
    }
    format!("{:x}", digest.finalize())
}
