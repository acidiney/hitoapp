use crate::api::artist_identity_groups::ArtistIdentityGroupPage;
use crate::api::browsing::{ArtistPage, ReleasePage};
use crate::api::catalog::RecordingPage;
use hito_core::{
    Activity, AudiobookDetail, AudiobookPagination, Library, LockStatus, Playlist, PlaylistDetail,
    PodcastEpisodePage, PodcastSubscription, RecordingProgress, User,
};
use hito_core::{
    ArtistDetail, ArtistIdentityGroupDetail, RecordingMetadata, ReleaseDetail, ReleaseMetadata,
};
use serde::Serialize;
use std::collections::BTreeMap;
use utoipa::ToSchema;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, ToSchema)]
#[allow(clippy::enum_variant_names)]
pub enum PageComponent {
    LoginPage,
    LockPage,
    HomePage,
    SearchPage,
    CatalogPage,
    AlbumIndexPage,
    AlbumPage,
    ArtistIndexPage,
    ArtistIdentityGroupPage,
    ArtistPage,
    RecordingDetailsPage,
    AlbumDetailsPage,
    LikesPage,
    PlaylistIndexPage,
    PlaylistPage,
    AudiobookIndexPage,
    AudiobookPage,
    HistoryPage,
    SettingsPage,
    QueuePage,
    DiscoverPage,
    PodcastIndexPage,
    PodcastPage,
    NotFoundPage,
}

impl std::fmt::Display for PageComponent {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{self:?}")
    }
}

#[derive(Serialize, ToSchema)]
pub struct SharedPageProps {
    pub errors: BTreeMap<String, String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schema(required = true, nullable = false)]
    pub native_playback: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schema(required = true, nullable = false)]
    pub user: Option<User>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schema(required = true, nullable = false)]
    pub libraries: Option<Vec<Library>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schema(required = true, nullable = false)]
    pub playlists: Option<Vec<Playlist>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schema(required = true, nullable = false)]
    pub lock: Option<LockStatus>,
}

#[derive(Serialize, ToSchema)]
pub struct LockPageProps {
    pub errors: BTreeMap<String, String>,
    pub locked: bool,
    pub username: String,
    pub account_id: String,
}

#[derive(Serialize, ToSchema)]
pub struct LoginPageProps {
    pub errors: BTreeMap<String, String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schema(required = true, nullable = false)]
    pub needs_setup: Option<bool>,
}

#[derive(Serialize, ToSchema)]
pub struct HomePageProps {
    #[serde(flatten)]
    pub shared: SharedPageProps,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schema(required = true, nullable = false)]
    pub home: Option<hito_core::Home>,
}

#[derive(Serialize, ToSchema)]
pub struct SearchPageProps {
    #[serde(flatten)]
    pub shared: SharedPageProps,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schema(required = true, nullable = false)]
    pub query: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schema(required = true, nullable = false)]
    pub recordings: Option<RecordingPage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schema(required = true, nullable = false)]
    pub releases: Option<ReleasePage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schema(required = true, nullable = false)]
    pub artists: Option<ArtistPage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schema(required = true, nullable = false)]
    pub connections: Option<Vec<hito_core::PluginInstallation>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schema(required = true, nullable = false)]
    pub service_names: Option<BTreeMap<String, String>>,
}

#[derive(Serialize, ToSchema)]
pub struct CatalogPageProps {
    #[serde(flatten)]
    pub shared: SharedPageProps,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schema(required = true, nullable = false)]
    pub query: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schema(required = true, nullable = false)]
    pub recordings: Option<RecordingPage>,
}

#[derive(Serialize, ToSchema)]
pub struct AlbumIndexPageProps {
    #[serde(flatten)]
    pub shared: SharedPageProps,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schema(required = true, nullable = false)]
    pub query: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schema(required = true, nullable = false)]
    pub releases: Option<ReleasePage>,
}

#[derive(Serialize, ToSchema)]
pub struct ArtistIndexPageProps {
    #[serde(flatten)]
    pub shared: SharedPageProps,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schema(required = true, nullable = false)]
    pub query: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schema(required = true, nullable = false)]
    pub artists: Option<ArtistPage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schema(required = true, nullable = false)]
    pub identity_groups: Option<ArtistIdentityGroupPage>,
}

#[derive(Serialize, ToSchema)]
pub struct ArtistIdentityGroupPageProps {
    #[serde(flatten)]
    pub shared: SharedPageProps,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schema(required = true, nullable = false)]
    pub detail: Option<ArtistIdentityGroupDetail>,
}

#[derive(Serialize, ToSchema)]
pub struct AlbumPageProps {
    #[serde(flatten)]
    pub shared: SharedPageProps,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schema(required = true, nullable = false)]
    pub detail: Option<ReleaseDetail>,
}

#[derive(Serialize, ToSchema)]
pub struct ArtistPageProps {
    #[serde(flatten)]
    pub shared: SharedPageProps,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schema(required = true, nullable = false)]
    pub detail: Option<ArtistDetail>,
}

#[derive(Serialize, ToSchema)]
pub struct RecordingDetailsPageProps {
    #[serde(flatten)]
    pub shared: SharedPageProps,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schema(required = true, nullable = false)]
    pub detail: Option<RecordingMetadata>,
}

#[derive(Serialize, ToSchema)]
pub struct AlbumDetailsPageProps {
    #[serde(flatten)]
    pub shared: SharedPageProps,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schema(required = true, nullable = false)]
    pub detail: Option<ReleaseMetadata>,
}

#[derive(Serialize, ToSchema)]
pub struct PlaylistIndexPageProps {
    #[serde(flatten)]
    pub shared: SharedPageProps,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schema(required = true, nullable = false)]
    pub connections: Option<Vec<hito_core::PluginInstallation>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schema(required = true, nullable = false)]
    pub service_names: Option<BTreeMap<String, String>>,
}

#[derive(Serialize, ToSchema)]
pub struct PlaylistPageProps {
    #[serde(flatten)]
    pub shared: SharedPageProps,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schema(required = true, nullable = false)]
    pub detail: Option<PlaylistDetail>,
}

#[derive(Serialize, ToSchema)]
pub struct AudiobookIndexPageProps {
    #[serde(flatten)]
    pub shared: SharedPageProps,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schema(required = true, nullable = false)]
    pub books: Option<Vec<Playlist>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schema(required = true, nullable = false)]
    pub book_pagination: Option<AudiobookPagination>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schema(required = true, nullable = false)]
    pub grouping_playlists: Option<Vec<Playlist>>,
}

#[derive(Serialize, ToSchema)]
pub struct AudiobookPageProps {
    #[serde(flatten)]
    pub shared: SharedPageProps,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schema(required = true, nullable = false)]
    pub detail: Option<AudiobookDetail>,
}

#[derive(Serialize, ToSchema)]
pub struct HistoryPageProps {
    #[serde(flatten)]
    pub shared: SharedPageProps,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schema(required = true, nullable = false)]
    pub activity: Option<Activity>,
}

#[derive(Serialize, ToSchema)]
pub struct SettingsPageProps {
    #[serde(flatten)]
    pub shared: SharedPageProps,
}

#[derive(Serialize, ToSchema)]
pub struct NotFoundPageProps {
    #[serde(flatten)]
    pub shared: SharedPageProps,
}

#[derive(Serialize, ToSchema)]
pub struct QueuePageProps {
    #[serde(flatten)]
    pub shared: SharedPageProps,
}

#[derive(Serialize, ToSchema)]
pub struct DiscoverPageProps {
    #[serde(flatten)]
    pub shared: SharedPageProps,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schema(required = true, nullable = false)]
    pub connections: Option<Vec<hito_core::PluginInstallation>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schema(required = true, nullable = false)]
    pub service_names: Option<BTreeMap<String, String>>,
}

#[derive(Serialize, ToSchema)]
pub struct PodcastIndexPageProps {
    #[serde(flatten)]
    pub shared: SharedPageProps,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schema(required = true, nullable = false)]
    pub subscriptions: Option<Vec<PodcastSubscription>>,
}

#[derive(Serialize, ToSchema)]
pub struct PodcastPageProps {
    #[serde(flatten)]
    pub shared: SharedPageProps,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schema(required = true, nullable = false)]
    pub subscription: Option<PodcastSubscription>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schema(required = true, nullable = false)]
    pub episodes: Option<PodcastEpisodePage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schema(required = true, nullable = false)]
    pub progress: Option<Vec<RecordingProgress>>,
}

#[derive(Serialize)]
#[serde(untagged)]
pub enum PageProps {
    Login(LoginPageProps),
    Lock(LockPageProps),
    Home(HomePageProps),
    Search(SearchPageProps),
    Catalog(CatalogPageProps),
    AlbumIndex(AlbumIndexPageProps),
    Album(AlbumPageProps),
    ArtistIndex(ArtistIndexPageProps),
    ArtistIdentityGroup(ArtistIdentityGroupPageProps),
    Artist(ArtistPageProps),
    RecordingDetails(Box<RecordingDetailsPageProps>),
    AlbumDetails(AlbumDetailsPageProps),
    Likes(CatalogPageProps),
    PlaylistIndex(PlaylistIndexPageProps),
    Playlist(PlaylistPageProps),
    AudiobookIndex(AudiobookIndexPageProps),
    Audiobook(AudiobookPageProps),
    History(HistoryPageProps),
    Settings(SettingsPageProps),
    Queue(QueuePageProps),
    Discover(DiscoverPageProps),
    PodcastIndex(PodcastIndexPageProps),
    Podcast(PodcastPageProps),
    NotFound(NotFoundPageProps),
}

impl PageProps {
    pub fn component(&self) -> PageComponent {
        match self {
            Self::Login(_) => PageComponent::LoginPage,
            Self::Lock(_) => PageComponent::LockPage,
            Self::Home(_) => PageComponent::HomePage,
            Self::Search(_) => PageComponent::SearchPage,
            Self::Catalog(_) => PageComponent::CatalogPage,
            Self::AlbumIndex(_) => PageComponent::AlbumIndexPage,
            Self::Album(_) => PageComponent::AlbumPage,
            Self::ArtistIndex(_) => PageComponent::ArtistIndexPage,
            Self::ArtistIdentityGroup(_) => PageComponent::ArtistIdentityGroupPage,
            Self::Artist(_) => PageComponent::ArtistPage,
            Self::RecordingDetails(_) => PageComponent::RecordingDetailsPage,
            Self::AlbumDetails(_) => PageComponent::AlbumDetailsPage,
            Self::Likes(_) => PageComponent::LikesPage,
            Self::PlaylistIndex(_) => PageComponent::PlaylistIndexPage,
            Self::Playlist(_) => PageComponent::PlaylistPage,
            Self::AudiobookIndex(_) => PageComponent::AudiobookIndexPage,
            Self::Audiobook(_) => PageComponent::AudiobookPage,
            Self::History(_) => PageComponent::HistoryPage,
            Self::Settings(_) => PageComponent::SettingsPage,
            Self::Queue(_) => PageComponent::QueuePage,
            Self::Discover(_) => PageComponent::DiscoverPage,
            Self::PodcastIndex(_) => PageComponent::PodcastIndexPage,
            Self::Podcast(_) => PageComponent::PodcastPage,
            Self::NotFound(_) => PageComponent::NotFoundPage,
        }
    }
}
