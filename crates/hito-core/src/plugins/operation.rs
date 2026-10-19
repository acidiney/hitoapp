use super::Plugins;
use crate::{Actor, Error, Result};
use hito_plugin_sdk::{
    Capability, Manifest, ProviderRecording, RecordingMetadata, RecordingRequest, SearchPage,
    SearchRequest, SourcePage,
};
use sqlx::Row;
use std::collections::BTreeSet;
use zeroize::Zeroizing;

mod timed_content;

pub(crate) struct Operation {
    pub(crate) provider: String,
    actor: Actor,
    account: String,
    installation: String,
    installation_revision: i64,
    approved_capabilities: BTreeSet<Capability>,
    pub(crate) service: String,
    pub(crate) service_revision: i64,
    base_url: String,
    token: Option<Zeroizing<String>>,
    remote: super::transport::ControlClient,
    media: super::media::MediaClient,
    store: crate::Store,
    credential: Option<Vec<u8>>,
    audio_generation: i64,
}
impl Plugins {
    pub(crate) async fn source_batch(
        &self,
        actor: &Actor,
        bindings: usize,
    ) -> Result<super::SourceBatch> {
        actor.check_session(&self.store).await?;
        self.remote.source_batch(actor.id(), bindings)
    }

    #[tracing::instrument(level = "debug", skip_all, err(Debug), fields(account_id = actor.id()))]
    pub(crate) async fn operation(
        &self,
        actor: &Actor,
        id: &str,
        expected_revision: i64,
        capabilities: &[Capability],
    ) -> Result<Operation> {
        let row = sqlx::query("SELECT p.revision,p.service_id,p.manifest,p.credential,p.audio_generation,s.revision AS service_revision,s.base_url
            FROM plugin_installations p JOIN plugin_services s ON s.id = p.service_id AND s.active = 1
            WHERE p.id = ? AND p.user_id = ? AND p.active = 1")
            .bind(id).bind(actor.id()).fetch_optional(&self.store.pool).await?.ok_or(Error::NotFound)?;
        let revision: i64 = row.try_get("revision")?;
        if revision != expected_revision {
            return Err(Error::Conflict(
                "This plugin connection changed. Reload it before continuing.".into(),
            ));
        }
        let manifest: Manifest = serde_json::from_str(row.try_get("manifest")?)
            .map_err(|_| Error::Internal("Stored plugin manifest is invalid".into()))?;
        if !capabilities
            .iter()
            .all(|capability| manifest.capabilities.contains(capability))
        {
            return Err(Error::Invalid(
                "This plugin has not been approved for the requested capability".into(),
            ));
        }
        let service: String = row.try_get("service_id")?;
        let credential: Option<Vec<u8>> = row.try_get("credential")?;
        let token = credential
            .as_deref()
            .map(|blob| self.vault.reveal(actor.id(), id, &service, blob))
            .transpose()?;
        Ok(Operation {
            provider: manifest.id,
            actor: actor.clone(),
            account: actor.id().into(),
            installation: id.into(),
            installation_revision: revision,
            approved_capabilities: manifest.capabilities,
            service,
            service_revision: row.try_get("service_revision")?,
            base_url: row.try_get("base_url")?,
            token,
            remote: self.remote.clone(),
            media: self.media.clone(),
            store: self.store.clone(),
            credential,
            audio_generation: row.try_get("audio_generation")?,
        })
    }
}
impl Operation {
    pub(crate) fn catalog_installation_revision(&self) -> i64 {
        self.installation_revision
    }

    pub(crate) async fn catalog_discovery_supported(&self) -> Result<bool> {
        self.check_catalog().await?;
        let result = self
            .remote
            .catalog_discovery_supported_admitted(
                &self.base_url,
                self.token.as_deref().map(String::as_str),
            )
            .await;
        self.check_catalog().await?;
        result
    }

    pub(crate) async fn catalog_artist_search(
        &self,
        request: &hito_plugin_sdk::ArtistSearchRequest,
    ) -> Result<hito_plugin_sdk::ArtistSearchPage> {
        self.check_catalog().await?;
        let result = self
            .remote
            .catalog_artist_search_admitted(
                &self.base_url,
                self.token.as_deref().map(String::as_str),
                request,
            )
            .await;
        self.check_catalog().await?;
        result
    }

    pub(crate) async fn catalog_profile_supported(&self) -> Result<bool> {
        self.check_catalog().await?;
        let result = self
            .remote
            .catalog_profile_supported_admitted(
                &self.base_url,
                self.token.as_deref().map(String::as_str),
            )
            .await;
        self.check_catalog().await?;
        result
    }

    pub(crate) async fn catalog_artist_profile(
        &self,
        request: &hito_plugin_sdk::ArtistProfileRequest,
    ) -> Result<hito_plugin_sdk::CatalogArtistProfile> {
        self.check_catalog().await?;
        let result = self
            .remote
            .catalog_artist_profile_admitted(
                &self.base_url,
                self.token.as_deref().map(String::as_str),
                request,
            )
            .await;
        self.check_catalog().await?;
        result
    }

    pub(crate) async fn catalog_discography_supported(&self) -> Result<bool> {
        self.check_catalog().await?;
        let result = self
            .remote
            .catalog_discography_supported_admitted(
                &self.base_url,
                self.token.as_deref().map(String::as_str),
            )
            .await;
        self.check_catalog().await?;
        result
    }

    pub(crate) async fn catalog_artist_discography(
        &self,
        request: &hito_plugin_sdk::ArtistDiscographyRequest,
    ) -> Result<hito_plugin_sdk::ArtistDiscographyPage> {
        self.check_catalog().await?;
        let result = self
            .remote
            .catalog_artist_discography_admitted(
                &self.base_url,
                self.token.as_deref().map(String::as_str),
                request,
            )
            .await;
        self.check_catalog().await?;
        result
    }

    async fn check_catalog(&self) -> Result<()> {
        if !self.approved_capabilities.contains(&Capability::Catalog) {
            return Err(Error::Invalid(
                "This plugin has not been approved for artist catalogs".into(),
            ));
        }
        let mut connection = self.store.pool.acquire().await?;
        self.actor.check_session_on(&mut connection).await?;
        self.recheck(&mut connection).await
    }

    pub(crate) async fn artwork(
        &self,
        request: &hito_plugin_sdk::ArtworkRequest,
    ) -> Result<Option<super::ProviderArtwork>> {
        self.remote
            .artwork(
                &self.account,
                &self.base_url,
                self.token.as_deref().map(String::as_str),
                request,
            )
            .await
    }

    pub(crate) async fn playlist_snapshot(
        &self,
        reference: String,
    ) -> Result<super::PlaylistSnapshot> {
        self.remote
            .playlist_snapshot(
                &self.account,
                &self.base_url,
                self.token.as_deref().map(String::as_str),
                reference,
            )
            .await
    }
    pub(crate) fn audio_authority(
        &self,
        source: &str,
        remote_source: &str,
        binding: &str,
        binding_revision: i64,
    ) -> super::AudioAuthority {
        super::media::Authority {
            store: self.store.clone(),
            account: self.account.clone(),
            installation: self.installation.clone(),
            service: self.service.clone(),
            credential: self.credential.clone(),
            source: source.into(),
            remote_source: remote_source.into(),
            generation: self.audio_generation,
            binding: binding.into(),
            binding_revision,
        }
    }
    pub(crate) async fn audio(
        &self,
        source: &str,
        remote_source: &str,
        binding: &str,
        binding_revision: i64,
        request: crate::AudioRequest,
    ) -> Result<crate::OpenedAudio> {
        let authority = self.audio_authority(source, remote_source, binding, binding_revision);
        authority.check().await?;
        let audio = self
            .media
            .open(
                &self.base_url,
                self.token.as_deref().map(String::as_str),
                request,
                authority,
            )
            .await?;
        let mut connection = self.store.pool.acquire().await?;
        self.recheck(&mut connection).await?;
        Ok(audio)
    }
    pub(crate) async fn recording(
        &self,
        request: &RecordingRequest,
    ) -> Result<(ProviderRecording, SourcePage)> {
        let details = self.recording_details(request).await?;
        {
            let mut connection = self.store.pool.acquire().await?;
            self.actor.check_session_on(&mut connection).await?;
            self.recheck(&mut connection).await?;
        }
        let sources = self.sources(request).await?;
        Ok((details, sources))
    }
    pub(crate) async fn recording_details(
        &self,
        request: &RecordingRequest,
    ) -> Result<ProviderRecording> {
        self.remote
            .recording_details(
                &self.account,
                &self.base_url,
                self.token.as_deref().map(String::as_str),
                request,
            )
            .await
    }
    pub(crate) fn metadata_approved(&self) -> bool {
        self.approved_capabilities.contains(&Capability::Metadata)
    }
    pub(crate) async fn recording_metadata(
        &self,
        request: &RecordingRequest,
    ) -> Result<RecordingMetadata> {
        if !self.metadata_approved() {
            return Err(Error::Invalid(
                "This plugin has not been approved for musical metadata".into(),
            ));
        }
        {
            let mut connection = self.store.pool.acquire().await?;
            self.actor.check_session_on(&mut connection).await?;
            self.recheck(&mut connection).await?;
        }
        let result = self
            .remote
            .recording_metadata(
                &self.account,
                &self.base_url,
                self.token.as_deref().map(String::as_str),
                request,
            )
            .await;
        let mut connection = self.store.pool.acquire().await?;
        self.actor.check_session_on(&mut connection).await?;
        self.recheck(&mut connection).await?;
        result
    }
    pub(crate) async fn search(&self, request: &SearchRequest) -> Result<SearchPage> {
        self.remote
            .search(
                &self.account,
                &self.base_url,
                self.token.as_deref().map(String::as_str),
                request,
            )
            .await
    }
    pub(crate) async fn sources(&self, request: &RecordingRequest) -> Result<SourcePage> {
        self.remote
            .sources(
                &self.account,
                &self.base_url,
                self.token.as_deref().map(String::as_str),
                request,
            )
            .await
    }
    pub(crate) async fn sources_in_batch(
        &self,
        batch: &super::SourceBatch,
        request: &RecordingRequest,
    ) -> Result<SourcePage> {
        self.remote
            .sources_in_batch(
                batch,
                &self.account,
                &self.base_url,
                self.token.as_deref().map(String::as_str),
                request,
            )
            .await
    }
    pub(crate) async fn recheck(&self, connection: &mut sqlx::SqliteConnection) -> Result<()> {
        let current: Option<(i64, i64)> = sqlx::query_as("SELECT p.revision,s.revision FROM plugin_installations p JOIN plugin_services s ON s.id = p.service_id AND s.active = 1
            WHERE p.id = ? AND p.user_id = ? AND p.service_id = ? AND p.active = 1")
            .bind(&self.installation).bind(&self.account).bind(&self.service).fetch_optional(connection).await?;
        let current = current.ok_or(Error::NotFound)?;
        if current != (self.installation_revision, self.service_revision) {
            return Err(Error::Conflict(
                "The plugin connection changed during the request. Try again.".into(),
            ));
        }
        Ok(())
    }
}
