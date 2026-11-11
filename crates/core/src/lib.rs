mod admission;
mod artist_catalog;
mod catalog;
mod catalog_replication;
mod collection;
mod community;
mod context;
mod downloads;
mod error;
mod id3_frames;
mod identity;
mod library_storage;
mod maintenance;
mod media_command;
mod metadata;
mod playback_access;
mod plugins;
mod podcasts;
mod private_file;
mod remote_http;
mod replicas;
mod secrets;
mod store;

#[cfg(any(test, feature = "test-support"))]
pub mod test_support;

pub use artist_catalog::{
    AlbumEditionChoice, AlbumEditionChoiceReceipt, AlbumEditionChoiceState,
    AlbumEditionTrackListAvailability, AlbumLibraryLevel, AlbumTrackAudio,
    ApplyArtistIdentityToCredit, ArtistAssociation, ArtistAssociationChangeReceipt,
    ArtistAssociationChanges, ArtistAssociationEvidence, ArtistAssociationRetirement,
    ArtistAssociationUndo, ArtistCandidatePreviewAvailability, ArtistCandidateReleasesAvailability,
    ArtistCandidateSearchAvailability, ArtistCatalog, ArtistCorrectionRetirement, ArtistCreditRole,
    ArtistDiscographyAvailability, ArtistIdentityChangeKind, ArtistIdentityChangeReceipt,
    ArtistIdentityChangeSummary, ArtistIdentityChoiceReceipt, ArtistIdentityCreditReceipt,
    ArtistIdentityGroup, ArtistIdentityGroupBrowse, ArtistIdentityGroupChanges,
    ArtistIdentityGroupDetail, ArtistIdentityGroupLibrary, ArtistIdentityGroupMember,
    ArtistIdentityGroupName, ArtistIdentityGroupQuery, ArtistIdentitySelection,
    ArtistMatchEvidence, ArtistMatchReceipt, ArtistMatchSuggestion, ArtistMatchSuggestions,
    ArtistMatchingState, ArtistMatchingStatus, ArtistProfileAvailability,
    ArtistProviderMatchReceipt, ArtistRadio, ArtistReviewCandidate, ArtistReviewConfirmation,
    ArtistReviewConnection, ArtistReviewContext, ArtistReviewDismissal, ArtistReviewItem,
    ArtistReviewMetadataBinding, ArtistReviewOrigin, ArtistReviewQueueQuery,
    ArtistReviewRestoration, AssociateRecordingArtist, CatalogAdoption, CatalogTrackMatch,
    ChooseAlbumEdition, ChooseRecordingArtistIdentity, ClearAlbumEdition,
    ConfirmArtistReviewCandidate, CoverageEvidence, CreditEvidence, CreditEvidenceKind,
    DiscographyLibrary, DismissArtistMatchSuggestion, DismissArtistReviewCandidates,
    EditionTrackListAvailability, FindAlbumTrackAudio, FindArtistTrackAudio, LibraryAlbum,
    LibraryLevel, LibraryReleaseGroup, MatchArtistsFromProviderEvidence, MatchArtistsFromTags,
    MetadataProvider, MetadataSettings, PromoteArtistCorrection, PromotionSource,
    ReadAlbumEditionTrackList, ReadAlbumLibraryLevel, ReadArtistDiscography,
    ReadDiscographyLibrary, ReadEditionTrackList, ReadReleaseGroupEditions, ReadSimilarArtists,
    RecordingArtistIdentityChoice, RecordingArtistIdentityState, ReleaseGroupEditionsAvailability,
    ResetRecordingArtistIdentity, RestoreArtistMatchSuggestion, RestoreArtistReviewCandidates,
    SavedAlbumEdition, SharedArtistCorrection, SharedArtistCorrectionQuery,
    SharedArtistCorrectionReceipt, SharedArtistCorrections, SharedCorrectionKind,
    SharedCorrectionRef, SimilarArtistView, SimilarArtistsAvailability, SplitArtistIdentityGroup,
    SplitDestination, TaggedAlbumEdition, UndoArtistIdentityChange, UndoRecordingArtistChange,
    UndoSharedArtistCorrection, UnlinkRecordingArtist, UpdateAutomaticLookups,
    UpdateMetadataSettings,
};
pub use catalog::{
    AdoptProviderRecording, AlbumArtistInference, AlbumCatalogAnchor, Artist, ArtistDetail,
    Artwork, ArtworkAvailability, ArtworkFetch, ArtworkFetchStatus, AttachReviewedProviderSource,
    Browse, Catalog, CatalogSort, ChooseSource, CoverArtKind, CoverArtLookup, CreditMember,
    FeaturedEntry, FeaturedShelf, Genre, GenreDetail, LyricsLookup, Page, PlaylistArtworkFetch,
    PrepareSource, ProviderCandidate, ProviderSearch, ProviderSourceReview, ReadRecordingLyrics,
    Recording, RecordingCopies, RecordingCopy, RecordingGroupProof, RecordingLyricsProvider,
    RecordingLyricsProviders, RecordingSplit, Release, ReleaseArtworkFetch, ReleaseDetail,
    ReleaseTrack, ResetSourceChoice, ResolveSource, Search, Source, SourceChoice,
    SourceChoiceEntry, SourceChoiceMode, SourceChoiceReceipt, SourceChoiceResetReceipt,
    SourceMatchKind, SourcePreferenceEntry, SourcePreferences, SourcePreparationPhase,
    SourcePreparationReceipt, SourcePreparationStatus, SourceResolution, SourceResolutionOutcome,
    UpdateSourcePreferences,
};
pub use catalog_replication::{
    BeginCatalogSnapshot, CatalogReplication, CatalogSnapshot, CatalogSnapshotMode,
    CatalogSnapshotPage, LocalRecordingFact, LocalReleaseAppearance, LocalSourceFact,
    ServerRelayObservation,
};
pub use collection::{
    Action, Activity, AddedAlbum, AudiobookBookmark, AudiobookChapterAnchor, AudiobookContinuation,
    AudiobookDetail, AudiobookLibraryPage, AudiobookLibraryQuery, AudiobookPagination,
    AudiobookPlayback, AudiobookPlaybackAdmission, Collection, CollectionPublication,
    FollowProviderPlaylist, HistoryCleared, HistoryEntry, HistoryPreference, Home, HomeAlbum,
    ImportProviderPlaylist, ImportedPlaylistEntry, ImportedPlaylistEntryKind,
    ImportedPlaylistEntryOrigin, ImportedPlaylistMetadataRefresh, ImportedPlaylistOrigin,
    ImportedPlaylistRecording, ImportedPlaylistSnapshot, OpenedProviderPlaylist, Operation,
    Playlist, PlaylistDetail, PlaylistEntry, PlaylistFollow, PlaylistFollowInterval,
    PlaylistFollowReceipt, PlaylistFollowRefresh, PlaylistFollowStatus, PlaylistImportReceipt,
    PlaylistMetadataRefreshOrigin, RecordingProgress, RefreshFollowedPlaylist,
    RefreshImportedPlaylistMetadata, SavedPosition, SyncBatch, SyncEvent, SyncPage, SyncReceipt,
    UnheardOnAlbum, UpdateHistoryPreference,
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
pub use hito_plugin_sdk::CreditRole;
pub use identity::{
    AccountDeletion, AccountSummary, Actor, AuthSession, AvatarImage, ChangePassword, Credentials,
    DeleteAccount, Identity, Library, LockStatus, MAX_AVATAR_UPLOAD_BYTES, RemovePin,
    SESSION_SECONDS, SessionInfo, SessionsRevoked, SetPin, Unlock, UpdateDisplayName, User,
};
pub use library_storage::AudioFile;
#[cfg(feature = "test-support")]
pub use library_storage::WatchCounters;
pub use library_storage::{
    BrowseDirectories, ConnectFolder, DirectoryEntry, DirectoryPage, DirectoryRef,
    FolderWatchSettings, Job, JobFailure, LibraryStorage, MissingRecordingsRemoval, MusicFolder,
    SharedDirectory, WatchBackend, WatchMode, WatchStatus, WatchedFolder,
};
pub use maintenance::{MigrationOutcome, migrate_existing};
pub use metadata::{
    CreditedArtistView, InspectedTags, MetadataHistoryQuery, MetadataObservation,
    MusicBrainzArtistRole, MusicBrainzArtistTag, ObservationOrigin, ObservationPage, ObservedTag,
    OptionalTextMetadata, RecordingMetadata, ReleaseMetadata, ReleaseMusicBrainzIds, TagLocation,
    TextMetadata,
};
pub use playback_access::{
    AudioContent, AudioRequest, AudioStream, AudioValidators, BeginPlaybackLease, ByteRange,
    CandidatesEnd, ConversionStream, ConvertedAudio, MAX_CONVERSION_START, MAX_FALLBACK_ATTEMPTS,
    OpenedAudio, PcmBlock, PcmFormat, PcmOpening, PcmStream, PlaybackAccess, PlaybackCandidates,
    PlaybackLease, PlaybackSource, QualityAudio, QualityRequest, StreamPace, StreamQuality,
};
pub use plugins::{
    ConfiguredOffer, ConfiguredPluginService, Connections, InstallPlugin, OfferPluginService,
    PluginAuthorization, PluginAuthorizationCallback, PluginAuthorizationOutcome,
    PluginAuthorizationRedirect, PluginAuthorizationState, PluginAuthorizationStatus,
    PluginConnectionState, PluginConnectionStatus, PluginCredential, PluginDeclarationCheck,
    PluginHealth, PluginHealthState, PluginInstallation, PluginInstallationPreview, PluginRefresh,
    PluginService, PluginSettings, Plugins, PreviewPluginInstallation, ProviderAccess,
    RefreshPlugin,
};
pub use podcasts::{
    EpisodesWithProgress, PodcastEpisode, PodcastEpisodePage, PodcastEpisodeQuery,
    PodcastRefreshReceipt, PodcastSubscription, PodcastSubscriptionReceipt,
    PodcastUnsubscribeReceipt, Podcasts, RefreshPodcast, SubscribePodcast, UnsubscribePodcast,
};
pub use remote_http::PodcastNetworkConfig;
pub use replicas::{
    ConnectReplica, QueuedReplicaChange, ReplicaConnection, ReplicaConnectionState, ReplicaSession,
    ReplicaSyncOutcome, Replicas, SyncClientId,
};
use store::Store;

#[derive(Clone)]
pub struct Services {
    pub identity: Identity,
    pub catalog: Catalog,
    pub catalog_replication: CatalogReplication,
    pub artist_catalog: ArtistCatalog,
    pub playback: PlaybackAccess,
    pub collection: Collection,
    pub community: Community,
    pub storage: LibraryStorage,
    pub plugins: Plugins,
    pub downloads: Downloads,
    pub podcasts: Podcasts,
    pub replicas: Replicas,
    store: Store,
}

#[tracing::instrument(level = "info", skip_all, err(Debug), fields(host_device = config.host_device.as_str()))]
pub async fn open(config: CoreConfig) -> Result<Services> {
    let directories = library_storage::Directories::new(config.storage)?;
    #[cfg(not(any(test, feature = "test-support")))]
    let store = Store::open(&config.database).await?;
    #[cfg(any(test, feature = "test-support"))]
    let store = match config.database_owner.as_ref() {
        Some(owner) => Store::open_owned(&config.database, owner).await?,
        None => Store::open(&config.database).await?,
    };
    let media_tools = config.media_tools.clone();
    let storage = LibraryStorage {
        store: store.clone(),
        host: config.host_device.clone(),
        directories,
        data_dir: config
            .database
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
            .map_or_else(|| ".".into(), std::path::Path::to_path_buf),
        inspector: library_storage::FileInspector::new(config.media_tools),
        watching: config.folder_watching,
    };
    storage.initialize().await?;
    let managed =
        library_storage::ManagedFiles::open(&config.database, storage.inspector.clone()).await?;
    let vault = secrets::Vault::open(&store, &config.database).await?;
    #[cfg(any(test, feature = "test-support"))]
    let stream_pace = config.stream_pace;
    #[cfg(not(any(test, feature = "test-support")))]
    let stream_pace = StreamPace::DEFAULT;
    let plugins = Plugins::new(store.clone(), vault.clone(), stream_pace).await?;
    let catalog_replication =
        CatalogReplication::new(store.clone(), config.host_device.clone()).await?;
    let replicas = Replicas::new(store.clone(), vault.clone(), &config.database);
    if let Err(error) = replicas.retire_pending().await {
        tracing::warn!(
            event = "replica_retirement_pending",
            error_code = error.code(),
            "Replica file retirement will be retried"
        );
    }
    let community = Community::new(
        store.clone(),
        config.host_device.clone(),
        vault,
        config.community,
        config.community_rankings,
    )?;
    let podcast_remote = remote_http::RemoteHttp::new(&config.podcast_network);
    let local_files = catalog::local_files::LocalFiles {
        store: store.clone(),
        host: config.host_device.clone(),
        directories: storage.directories.clone(),
    };
    #[cfg(any(test, feature = "test-support"))]
    let builtin_metadata = match config.musicbrainz_loopback_endpoint.clone() {
        Some(endpoint) => {
            hito_metadata_musicbrainz::MusicBrainzClient::with_loopback_endpoint(endpoint)
        }
        None => {
            hito_metadata_musicbrainz::MusicBrainzClient::new(config.metadata_contact.as_deref())
        }
    };
    #[cfg(not(any(test, feature = "test-support")))]
    let builtin_metadata =
        hito_metadata_musicbrainz::MusicBrainzClient::new(config.metadata_contact.as_deref());
    let builtin_metadata = builtin_metadata.map_err(|_| {
        Error::Internal("The built-in metadata client could not be initialized".into())
    })?;
    let catalog = Catalog {
        community: community.clone(),
        plugins: plugins.clone(),
        store: store.clone(),
        host: config.host_device.clone(),
        artwork_slots: std::sync::Arc::new(tokio::sync::Semaphore::new(4)),
        provider_artwork_slots: std::sync::Arc::new(tokio::sync::Semaphore::new(4)),
        resolution_callers: Default::default(),
        featured_cache: Default::default(),
        local_files: local_files.clone(),
        cover_art: {
            #[cfg(any(test, feature = "test-support"))]
            let client = match config.cover_art_loopback_endpoint.clone() {
                Some(endpoint) => {
                    hito_metadata_musicbrainz::CoverArtClient::with_loopback_endpoint(endpoint)
                }
                None => hito_metadata_musicbrainz::CoverArtClient::new(
                    config.metadata_contact.as_deref(),
                ),
            };
            #[cfg(not(any(test, feature = "test-support")))]
            let client =
                hito_metadata_musicbrainz::CoverArtClient::new(config.metadata_contact.as_deref());
            client.map_err(|_| {
                Error::Internal("The cover art client could not be initialized".into())
            })?
        },
        artist_image: {
            #[cfg(any(test, feature = "test-support"))]
            let client = match config.artist_image_loopback_endpoint.clone() {
                Some(endpoint) => {
                    hito_metadata_musicbrainz::ArtistImageClient::with_loopback_endpoint(
                        builtin_metadata.clone(),
                        endpoint,
                    )
                }
                None => hito_metadata_musicbrainz::ArtistImageClient::new(
                    builtin_metadata.clone(),
                    config.metadata_contact.as_deref(),
                ),
            };
            #[cfg(not(any(test, feature = "test-support")))]
            let client = hito_metadata_musicbrainz::ArtistImageClient::new(
                builtin_metadata.clone(),
                config.metadata_contact.as_deref(),
            );
            client.map_err(|_| {
                Error::Internal("The artist image client could not be initialized".into())
            })?
        },
    };
    let artist_catalog = ArtistCatalog::new(
        catalog.clone(),
        store.clone(),
        plugins.clone(),
        config.host_device.clone(),
        builtin_metadata,
    )?;
    let playback = PlaybackAccess {
        leases: Default::default(),
        store: store.clone(),
        host: config.host_device.clone(),
        origins: playback_access::Origins::new(
            playback_access::ProviderOrigin {
                store: store.clone(),
                plugins: plugins.clone(),
            },
            std::sync::Arc::new(podcasts::EnclosureOrigin::new(
                store.clone(),
                podcast_remote.clone(),
            )),
            std::sync::Arc::new(downloads::ManagedOrigin::new(
                store.clone(),
                config.host_device.clone(),
                managed.clone(),
            )),
            std::sync::Arc::new(playback_access::LocalOrigin {
                files: local_files.clone(),
            }),
        ),
        conversions: playback_access::ConversionLimits::new(config.conversion_parallelism),
        tools: media_tools,
    };
    tracing::info!("Core services ready");
    let downloads = Downloads {
        store: store.clone(),
        plugins: plugins.clone(),
        remote: podcast_remote.clone(),
        host: config.host_device.clone(),
        files: managed.clone(),
        active: Default::default(),
        playlist_turn: Default::default(),
    };
    if let Err(error) = downloads.sweep_staging().await {
        tracing::warn!(
            event = "download_staging_sweep_failed",
            error_code = error.code(),
            "Leftover download staging files could not be removed"
        );
    }
    Ok(Services {
        identity: Identity {
            store: store.clone(),
            replicas: replicas.clone(),
            catalog_replication: catalog_replication.clone(),
            password_attempts: Default::default(),
            seen_attempts: Default::default(),
        },
        community,
        catalog_replication,
        replicas,
        podcasts: Podcasts {
            store: store.clone(),
            host: config.host_device.clone(),
            remote: podcast_remote.clone(),
        },
        catalog,
        artist_catalog,
        playback,
        collection: Collection {
            store: store.clone(),
            host: config.host_device.clone(),
            plugins: plugins.clone(),
            metadata_refresh_admission: Default::default(),
        },
        storage,
        downloads,
        plugins,
        store,
    })
}

impl Services {
    pub async fn healthy(&self) -> bool {
        self.store.healthy().await
    }
}
