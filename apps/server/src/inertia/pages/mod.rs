mod audiobooks;
mod catalog;
mod discover;
mod genres;
mod home;
mod playlists;
mod podcasts;
mod search;

use super::{ArtistPagination, props::*, protocol::Protocol};
use crate::error::ApiResult;
use hito_core::{Actor, Search, Services};
use std::future::Future;

pub enum Screen<'a> {
    Home,
    Search,
    Catalog,
    AlbumIndex,
    Album(&'a str),
    AlbumDetails(&'a str),
    ArtistIndex,
    ArtistIdentityGroup(&'a str),
    Artist(&'a str),
    RecordingDetails(&'a str),
    Likes,
    PlaylistIndex,
    Playlist(&'a str),
    AudiobookIndex,
    Audiobook(&'a str),
    History,
    Settings,
    Queue,
    Discover,
    Genre,
    PodcastIndex,
    Podcast(&'a str),
    NotFound,
}

impl<'a> Screen<'a> {
    pub fn is_unknown(&self) -> bool {
        matches!(self, Self::NotFound)
    }

    pub fn from_path(path: &'a str) -> Self {
        match path {
            "/" | "/library" => Self::Catalog,
            "/home" => Self::Home,
            "/search" => Self::Search,
            "/likes" => Self::Likes,
            "/playlists" => Self::PlaylistIndex,
            "/audiobooks" => Self::AudiobookIndex,
            "/history" => Self::History,
            "/settings" => Self::Settings,
            "/queue" => Self::Queue,
            "/discover" => Self::Discover,
            "/genres" => Self::Genre,
            "/podcasts" => Self::PodcastIndex,
            "/albums" => Self::AlbumIndex,
            "/artists" => Self::ArtistIndex,
            _ if path.starts_with("/artist-groups/") => {
                Self::ArtistIdentityGroup(path.trim_start_matches("/artist-groups/"))
            }
            _ if path.starts_with("/albums/") && path.ends_with("/details") => Self::AlbumDetails(
                path.trim_start_matches("/albums/")
                    .trim_end_matches("/details"),
            ),
            _ if path.starts_with("/albums/") => Self::Album(path.trim_start_matches("/albums/")),
            _ if path.starts_with("/artists/") => {
                Self::Artist(path.trim_start_matches("/artists/"))
            }
            _ if path.starts_with("/podcasts/") => {
                Self::Podcast(path.trim_start_matches("/podcasts/"))
            }
            _ if path.starts_with("/recordings/") => {
                Self::RecordingDetails(path.trim_start_matches("/recordings/"))
            }
            _ if path.starts_with("/audiobooks/") => {
                Self::Audiobook(path.trim_start_matches("/audiobooks/"))
            }
            _ if path.starts_with("/playlists/") => {
                Self::Playlist(path.trim_start_matches("/playlists/"))
            }
            _ => Self::NotFound,
        }
    }

    pub fn component(&self) -> PageComponent {
        match self {
            Self::Home => PageComponent::HomePage,
            Self::Search => PageComponent::SearchPage,
            Self::Catalog => PageComponent::CatalogPage,
            Self::AlbumIndex => PageComponent::AlbumIndexPage,
            Self::Album(_) => PageComponent::AlbumPage,
            Self::AlbumDetails(_) => PageComponent::AlbumDetailsPage,
            Self::ArtistIndex => PageComponent::ArtistIndexPage,
            Self::ArtistIdentityGroup(_) => PageComponent::ArtistIdentityGroupPage,
            Self::Artist(_) => PageComponent::ArtistPage,
            Self::RecordingDetails(_) => PageComponent::RecordingDetailsPage,
            Self::Likes => PageComponent::LikesPage,
            Self::PlaylistIndex => PageComponent::PlaylistIndexPage,
            Self::Playlist(_) => PageComponent::PlaylistPage,
            Self::AudiobookIndex => PageComponent::AudiobookIndexPage,
            Self::Audiobook(_) => PageComponent::AudiobookPage,
            Self::History => PageComponent::HistoryPage,
            Self::Settings => PageComponent::SettingsPage,
            Self::Queue => PageComponent::QueuePage,
            Self::Discover => PageComponent::DiscoverPage,
            Self::Genre => PageComponent::GenrePage,
            Self::PodcastIndex => PageComponent::PodcastIndexPage,
            Self::Podcast(_) => PageComponent::PodcastPage,
            Self::NotFound => PageComponent::NotFoundPage,
        }
    }

    pub async fn props(
        &self,
        services: &Services,
        actor: &Actor,
        search: Search,
        pagination: ArtistPagination,
        fields: &Protocol,
        native_playback: bool,
    ) -> ApiResult<PageProps> {
        let mut shared = shared(services, actor, fields).await?;
        shared.native_playback = fields
            .requested("native_playback")
            .then_some(native_playback);
        let request = PageRequest {
            services,
            actor,
            fields,
            search,
            pagination,
            shared,
        };
        match self {
            Self::Home => home::home(request).await,
            Self::Search => search::search(request).await,
            Self::Catalog => catalog::recordings(request).await,
            Self::Likes => catalog::likes(request).await,
            Self::AlbumIndex => catalog::album_index(request).await,
            Self::Album(id) => catalog::album(request, id).await,
            Self::AlbumDetails(id) => catalog::album_details(request, id).await,
            Self::ArtistIndex => catalog::artist_index(request).await,
            Self::ArtistIdentityGroup(id) => catalog::artist_identity_group(request, id).await,
            Self::Artist(id) => catalog::artist(request, id).await,
            Self::RecordingDetails(id) => catalog::recording_details(request, id).await,
            Self::PlaylistIndex => playlists::index(request).await,
            Self::Playlist(id) => playlists::detail(request, id).await,
            Self::History => playlists::history(request).await,
            Self::AudiobookIndex => audiobooks::index(request).await,
            Self::Audiobook(id) => audiobooks::detail(request, id).await,
            Self::PodcastIndex => podcasts::index(request).await,
            Self::Podcast(id) => podcasts::detail(request, id).await,
            Self::Discover => discover::discover(request).await,
            Self::Genre => genres::genre(request).await,
            Self::Settings => Ok(PageProps::Settings(SettingsPageProps {
                shared: request.shared,
            })),
            Self::Queue => Ok(PageProps::Queue(QueuePageProps {
                shared: request.shared,
            })),
            Self::NotFound => Ok(PageProps::NotFound(NotFoundPageProps {
                shared: request.shared,
            })),
        }
    }
}

struct PageRequest<'a> {
    services: &'a Services,
    actor: &'a Actor,
    fields: &'a Protocol,
    search: Search,
    pagination: ArtistPagination,
    shared: SharedPageProps,
}

impl PageRequest<'_> {
    fn requested(&self, key: &str) -> bool {
        self.fields.requested(key)
    }

    fn query(&self) -> Option<String> {
        self.requested("query")
            .then(|| self.search.q.clone().unwrap_or_default())
    }

    async fn when<T>(
        &self,
        key: &str,
        load: impl Future<Output = ApiResult<T>>,
    ) -> ApiResult<Option<T>> {
        if self.requested(key) {
            Ok(Some(load.await?))
        } else {
            Ok(None)
        }
    }
}

async fn shared(
    services: &Services,
    actor: &Actor,
    fields: &Protocol,
) -> ApiResult<SharedPageProps> {
    let libraries = if fields.requested("libraries") {
        Some(services.catalog.libraries(actor).await?)
    } else {
        None
    };
    let playlists = if fields.requested("playlists") {
        Some(services.collection.playlists(actor).await?)
    } else {
        None
    };
    let lock = if fields.requested("lock") {
        Some(services.identity.lock_status(actor).await?)
    } else {
        None
    };
    Ok(SharedPageProps {
        errors: Default::default(),
        lock,
        native_playback: None,
        user: fields.requested("user").then(|| actor.user().clone()),
        libraries,
        playlists,
    })
}
