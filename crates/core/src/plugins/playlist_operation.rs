use super::{PlaylistSnapshot, Plugins, transport::ControlClient};
use crate::{Error, Result};
use hito_plugin_sdk::{Capability, Manifest, PlaylistDetailRequest};
use sqlx::{Row, SqliteConnection};
use zeroize::Zeroizing;

pub(crate) struct PlaylistOperation {
    pub(crate) account: String,
    pub(crate) installation: String,
    pub(crate) installation_revision: i64,
    pub(crate) service: String,
    pub(crate) service_revision: i64,
    pub(crate) provider: String,
    base_url: String,
    token: Option<Zeroizing<String>>,
    remote: ControlClient,
}

pub(crate) enum PlaylistOperationFailure {
    RevisionChanged { expected: i64, current: i64 },
    Core(Error),
}

impl From<Error> for PlaylistOperationFailure {
    fn from(error: Error) -> Self {
        Self::Core(error)
    }
}

impl From<sqlx::Error> for PlaylistOperationFailure {
    fn from(error: sqlx::Error) -> Self {
        Self::Core(error.into())
    }
}

impl From<PlaylistOperationFailure> for Error {
    fn from(error: PlaylistOperationFailure) -> Self {
        match error {
            PlaylistOperationFailure::RevisionChanged { .. } => {
                Self::Conflict("This playlist connection changed".into())
            }
            PlaylistOperationFailure::Core(error) => error,
        }
    }
}

impl Plugins {
    pub(crate) async fn playlist_operation(
        &self,
        account: &str,
        installation: &str,
        expected_revision: i64,
    ) -> std::result::Result<PlaylistOperation, PlaylistOperationFailure> {
        let row = sqlx::query("SELECT p.revision,p.service_id,p.manifest,p.credential,s.revision AS service_revision,s.base_url
            FROM plugin_installations p JOIN plugin_services s ON s.id = p.service_id AND s.active = 1
            JOIN users account ON account.id = p.user_id
            WHERE p.id = ? AND p.user_id = ? AND p.active = 1")
            .bind(installation).bind(account).fetch_optional(&self.store.pool).await?.ok_or(Error::NotFound)?;
        let revision: i64 = row.try_get("revision")?;
        if revision != expected_revision {
            return Err(PlaylistOperationFailure::RevisionChanged {
                expected: expected_revision,
                current: revision,
            });
        }
        let manifest: Manifest = serde_json::from_str(row.try_get("manifest")?)
            .map_err(|_| Error::Internal("Stored plugin manifest is invalid".into()))?;
        if !manifest.capabilities.contains(&Capability::Playlists) {
            return Err(
                Error::Conflict("This connection is not approved for playlists".into()).into(),
            );
        }
        let service: String = row.try_get("service_id")?;
        let credential: Option<Vec<u8>> = row.try_get("credential")?;
        let token = credential
            .as_deref()
            .map(|blob| self.vault.reveal(account, installation, &service, blob))
            .transpose()?;
        Ok(PlaylistOperation {
            account: account.into(),
            installation: installation.into(),
            installation_revision: revision,
            service,
            service_revision: row.try_get("service_revision")?,
            provider: manifest.id,
            base_url: row.try_get("base_url")?,
            token,
            remote: self.remote.clone(),
        })
    }
}

impl PlaylistOperation {
    pub(crate) async fn recheck(&self, connection: &mut SqliteConnection) -> Result<()> {
        let current: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM plugin_installations p
            JOIN plugin_services s ON s.id = p.service_id AND s.active = 1 AND s.revision = ?
            JOIN users account ON account.id = p.user_id
            WHERE p.id = ? AND p.user_id = ? AND p.active = 1 AND p.revision = ? AND p.service_id = ?
                AND json_extract(p.manifest,'$.id') = ?
                AND EXISTS(SELECT 1 FROM json_each(p.manifest,'$.capabilities') WHERE value = 'playlists'))")
            .bind(self.service_revision).bind(&self.installation).bind(&self.account)
            .bind(self.installation_revision).bind(&self.service).bind(&self.provider)
            .fetch_one(connection).await?;
        if !current {
            return Err(Error::Conflict("This playlist connection changed".into()));
        }
        Ok(())
    }

    pub(crate) async fn snapshot<F, Check>(
        &self,
        request: PlaylistDetailRequest,
        check: F,
    ) -> Result<PlaylistSnapshot>
    where
        F: Fn() -> Check + Send + Sync,
        Check: std::future::Future<Output = Result<()>> + Send,
    {
        self.remote
            .playlist_snapshot_checked(
                &self.account,
                &self.base_url,
                self.token.as_deref().map(String::as_str),
                request,
                check,
            )
            .await
    }

    pub(crate) async fn refresh_snapshot<F, Check>(
        &self,
        request: PlaylistDetailRequest,
        check: F,
    ) -> Result<PlaylistSnapshot>
    where
        F: Fn() -> Check + Send + Sync,
        Check: std::future::Future<Output = Result<()>> + Send,
    {
        self.remote
            .follow_playlist_snapshot(
                &self.account,
                &self.base_url,
                self.token.as_deref().map(String::as_str),
                request,
                check,
            )
            .await
    }
}
