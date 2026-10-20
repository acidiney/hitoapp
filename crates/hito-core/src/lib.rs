mod admission;
mod catalog;
mod collection;
mod community;
mod context;
mod downloads;
mod error;
mod identity;
mod library_storage;
mod maintenance;
mod metadata;
mod plugins;
mod podcasts;
mod remote_http;
mod secrets;
mod store;

#[cfg(test)]
mod test_support;

pub use catalog::{
    AdoptProviderRecording, Artist, ArtistAssociation, ArtistAssociationEvidence,
    ArtistCandidatePreviewAvailability, ArtistCandidateSearchAvailability, ArtistCreditRole,
    ArtistDetail, ArtistDiscographyAvailability, ArtistIdentityChoiceReceipt, ArtistIdentityGroup,
    ArtistIdentityGroupBrowse, ArtistIdentityGroupDetail, ArtistIdentityGroupLibrary,
    ArtistIdentityGroupMember, ArtistIdentityGroupName, ArtistIdentityGroupQuery,
    ArtistIdentitySelection, ArtistProfileAvailability, ArtistReviewConnection,
    ArtistReviewContext, ArtistReviewMetadataBinding, Artwork, ArtworkAvailability, ArtworkFetch,
    ArtworkFetchStatus, AssociateRecordingArtist, AttachReviewedProviderSource, AudioContent,
    AudioFile, AudioRequest, AudioStream, Browse, ByteRange, Catalog,
    ChooseRecordingArtistIdentity, ChooseSource, LyricsLookup, MetadataProvider, MetadataSettings,
    OpenedAudio, Page, PlaybackCandidates, PlaybackSource, PlaylistArtworkFetch, PrepareSource,
    ProviderCandidate, ProviderSearch, ProviderSourceReview, ReadArtistDiscography,
    ReadRecordingLyrics, Recording, RecordingArtistIdentityChoice, RecordingArtistIdentityState,
    RecordingLyricsProvider, RecordingLyricsProviders, Release, ReleaseArtworkFetch, ReleaseDetail,
    ReleaseTrack, ResetRecordingArtistIdentity, ResetSourceChoice, ResolveSource, Search, Source,
    SourceChoice, SourceChoiceEntry, SourceChoiceMode, SourceChoiceReceipt,
    SourceChoiceResetReceipt, SourcePreparationPhase, SourcePreparationReceipt,
    SourcePreparationStatus, SourceResolution, SourceResolutionOutcome, UpdateMetadataSettings,
};
pub use collection::{
    Action, Activity, AudiobookBookmark, AudiobookChapterAnchor, AudiobookContinuation,
    AudiobookDetail, AudiobookLibraryPage, AudiobookLibraryQuery, AudiobookPagination,
    AudiobookPlayback, AudiobookPlaybackAdmission, Collection, CollectionPublication,
    FollowProviderPlaylist, HistoryEntry, ImportProviderPlaylist, ImportedPlaylistEntry,
    ImportedPlaylistEntryKind, ImportedPlaylistEntryOrigin, ImportedPlaylistMetadataRefresh,
    ImportedPlaylistOrigin, ImportedPlaylistRecording, ImportedPlaylistSnapshot, Operation,
    Playlist, PlaylistDetail, PlaylistEntry, PlaylistFollow, PlaylistFollowInterval,
    PlaylistFollowReceipt, PlaylistFollowRefresh, PlaylistFollowStatus, PlaylistImportReceipt,
    PlaylistMetadataRefreshOrigin, RecordingProgress, RefreshFollowedPlaylist,
    RefreshImportedPlaylistMetadata, SavedPosition, SyncBatch, SyncEvent, SyncPage, SyncReceipt,
};
pub use community::{
    Community, CommunityConfig, CommunityCredentialReset, CommunityPreferences,
    CommunityRankingConfig, CommunityRankingPreferences, CommunityReportAdmission,
    SuccessfulPlayback, UpdateCommunityPreferences, UpdateCommunityRankingPreferences,
};
pub use context::{CoreConfig, HostDeviceId, MediaTools, SharedDirectoryConfig, StorageConfig};
pub use downloads::{
    DownloadReceipt, DownloadRequest, DownloadState, DownloadStatus, Downloads,
    PlaylistDownloadEntries, PlaylistDownloadEntry, PlaylistDownloadEntryState,
    PlaylistDownloadQuery, PlaylistDownloadReceipt, PlaylistDownloadStatus,
};
pub use error::{Error, Result};
pub use identity::{Actor, AuthSession, Credentials, Identity, Library, User};
pub use library_storage::{
    BrowseDirectories, ConnectFolder, DirectoryEntry, DirectoryPage, DirectoryRef, Job,
    LibraryStorage, MusicFolder, SharedDirectory,
};
pub use maintenance::{MigrationOutcome, migrate_existing};
pub use metadata::{
    InspectedTags, MetadataHistoryQuery, MetadataObservation, MusicBrainzArtistRole,
    MusicBrainzArtistTag, ObservationOrigin, ObservationPage, ObservedTag, OptionalTextMetadata,
    RecordingMetadata, ReleaseMetadata, TagLocation, TextMetadata,
};
pub use plugins::{
    InstallPlugin, OfferPluginService, PluginAuthorization, PluginAuthorizationRedirect,
    PluginAuthorizationState, PluginAuthorizationStatus, PluginCredential, PluginInstallation,
    PluginInstallationPreview, PluginRefresh, PluginService, PluginSettings, Plugins,
    PreviewPluginInstallation, RefreshPlugin,
};
pub use podcasts::{
    PodcastEpisode, PodcastEpisodePage, PodcastEpisodeQuery, PodcastRefreshReceipt,
    PodcastSubscription, PodcastSubscriptionReceipt, PodcastUnsubscribeReceipt, Podcasts,
    RefreshPodcast, SubscribePodcast, UnsubscribePodcast,
};
pub use remote_http::PodcastNetworkConfig;
use store::Store;

#[derive(Clone)]
pub struct Services {
    pub identity: Identity,
    pub catalog: Catalog,
    pub collection: Collection,
    pub community: Community,
    pub storage: LibraryStorage,
    pub plugins: Plugins,
    pub downloads: Downloads,
    pub podcasts: Podcasts,
    store: Store,
}

#[tracing::instrument(level = "info", skip_all, err(Debug), fields(host_device = config.host_device.as_str()))]
pub async fn open(config: CoreConfig) -> Result<Services> {
    let directories = library_storage::Directories::new(config.storage)?;
    let store = Store::open(&config.database).await?;
    let storage = LibraryStorage {
        store: store.clone(),
        host: config.host_device.clone(),
        directories,
        inspector: library_storage::FileInspector::new(config.media_tools),
    };
    storage.initialize().await?;
    let managed =
        library_storage::ManagedFiles::open(&config.database, storage.inspector.clone()).await?;
    let vault = secrets::Vault::open(&store, &config.database).await?;
    let plugins = Plugins::new(store.clone(), vault.clone()).await?;
    let community = Community::new(
        store.clone(),
        config.host_device.clone(),
        vault,
        config.community,
        config.community_rankings,
    )?;
    let podcast_remote = remote_http::RemoteHttp::new(&config.podcast_network);
    let catalog = Catalog {
        community: community.clone(),
        plugins: plugins.clone(),
        artist_catalog: catalog::artist_catalog::Host::new()?,
        store: store.clone(),
        host: config.host_device.clone(),
        artwork_slots: std::sync::Arc::new(tokio::sync::Semaphore::new(4)),
        provider_artwork_slots: std::sync::Arc::new(tokio::sync::Semaphore::new(4)),
        resolution_callers: Default::default(),
        directories: storage.directories.clone(),
        managed: managed.clone(),
        podcast_remote: podcast_remote.clone(),
        podcast_audio_slots: std::sync::Arc::new(tokio::sync::Semaphore::new(16)),
        podcast_audio_accounts: admission::Accounts::new(2),
    };
    tracing::info!("Core services ready");
    Ok(Services {
        identity: Identity {
            store: store.clone(),
        },
        community,
        podcasts: Podcasts {
            store: store.clone(),
            catalog: catalog.clone(),
            remote: podcast_remote.clone(),
        },
        catalog,
        collection: Collection {
            store: store.clone(),
            host: config.host_device.clone(),
            plugins: plugins.clone(),
            metadata_refresh_admission: Default::default(),
        },
        storage,
        downloads: Downloads {
            store: store.clone(),
            plugins: plugins.clone(),
            remote: podcast_remote,
            host: config.host_device.clone(),
            files: managed.clone(),
            active: Default::default(),
            playlist_turn: Default::default(),
        },
        plugins,
        store,
    })
}

impl Services {
    pub async fn healthy(&self) -> bool {
        self.store.healthy().await
    }
}
