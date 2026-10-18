use super::{props::*, protocol::Protocol};
use crate::api::browsing::{ArtistPage, ReleasePage};
use crate::{api::catalog::RecordingPage, error::ApiResult};
use hito_core::{
    Actor, AudiobookLibraryQuery, Browse, Error, PodcastEpisodeQuery, Search, Services,
};
use uuid::Uuid;

pub enum Screen<'a> {
    Catalog,
    AlbumIndex,
    Album(&'a str),
    AlbumDetails(&'a str),
    ArtistIndex,
    Artist(&'a str),
    RecordingDetails(&'a str),
    Likes,
    PlaylistIndex,
    Playlist(&'a str),
    AudiobookIndex,
    Audiobook(&'a str),
    History,
    Settings,
    Discover,
    PodcastIndex,
    Podcast(&'a str),
}

impl<'a> Screen<'a> {
    pub fn from_path(path: &'a str) -> Self {
        match path {
            "/" | "/library" => Self::Catalog,
            "/likes" => Self::Likes,
            "/playlists" => Self::PlaylistIndex,
            "/audiobooks" => Self::AudiobookIndex,
            "/history" => Self::History,
            "/settings" => Self::Settings,
            "/discover" => Self::Discover,
            "/podcasts" => Self::PodcastIndex,
            "/albums" => Self::AlbumIndex,
            "/artists" => Self::ArtistIndex,
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
            _ => Self::Playlist(path.trim_start_matches("/playlists/")),
        }
    }

    pub fn component(&self) -> &'static str {
        match self {
            Self::Catalog => "CatalogPage",
            Self::AlbumIndex => "AlbumIndexPage",
            Self::Album(_) => "AlbumPage",
            Self::AlbumDetails(_) => "AlbumDetailsPage",
            Self::ArtistIndex => "ArtistIndexPage",
            Self::Artist(_) => "ArtistPage",
            Self::RecordingDetails(_) => "RecordingDetailsPage",
            Self::Likes => "LikesPage",
            Self::PlaylistIndex => "PlaylistIndexPage",
            Self::Playlist(_) => "PlaylistPage",
            Self::AudiobookIndex => "AudiobookIndexPage",
            Self::Audiobook(_) => "AudiobookPage",
            Self::History => "HistoryPage",
            Self::Settings => "SettingsPage",
            Self::Discover => "DiscoverPage",
            Self::PodcastIndex => "PodcastIndexPage",
            Self::Podcast(_) => "PodcastPage",
        }
    }

    pub async fn props(
        &self,
        services: &Services,
        actor: &Actor,
        mut search: Search,
        pagination: super::ArtistPagination,
        fields: &Protocol,
        native_playback: bool,
    ) -> ApiResult<PageProps> {
        let mut shared = shared(services, actor, fields).await?;
        shared.native_playback = fields
            .requested("native_playback")
            .then_some(native_playback);
        match self {
            Self::PodcastIndex => {
                let subscriptions = if fields.requested("subscriptions") {
                    Some(services.podcasts.subscriptions(actor).await?)
                } else {
                    None
                };
                Ok(PageProps::PodcastIndex(PodcastIndexPageProps {
                    shared,
                    subscriptions,
                }))
            }
            Self::Podcast(id) => {
                let subscription_id = Uuid::parse_str(id).map_err(|_| Error::NotFound)?;
                let subscription = services
                    .podcasts
                    .subscriptions(actor)
                    .await?
                    .into_iter()
                    .find(|subscription| subscription.subscription_id == subscription_id)
                    .ok_or(Error::NotFound)?;
                let episodes = if fields.requested("episodes") || fields.requested("progress") {
                    Some(
                        services
                            .podcasts
                            .episodes(actor, id, PodcastEpisodeQuery::default())
                            .await?,
                    )
                } else {
                    None
                };
                let progress = if fields.requested("progress") {
                    let recording_ids = episodes
                        .as_ref()
                        .into_iter()
                        .flat_map(|page| &page.items)
                        .map(|episode| {
                            Uuid::parse_str(&episode.recording.id).map_err(|_| {
                                Error::Internal("Stored recording identity is invalid".into())
                            })
                        })
                        .collect::<Result<Vec<_>, _>>()?;
                    Some(
                        services
                            .collection
                            .saved_playback(actor, &recording_ids)
                            .await?,
                    )
                } else {
                    None
                };
                Ok(PageProps::Podcast(PodcastPageProps {
                    shared,
                    subscription: fields.requested("subscription").then_some(subscription),
                    episodes: episodes.filter(|_| fields.requested("episodes")),
                    progress,
                }))
            }
            Self::AlbumDetails(id) => {
                let detail = if fields.requested("detail") {
                    Some(
                        services
                            .catalog
                            .release_metadata(actor, id, Default::default())
                            .await?,
                    )
                } else {
                    None
                };
                Ok(PageProps::AlbumDetails(AlbumDetailsPageProps {
                    shared,
                    detail,
                }))
            }
            Self::RecordingDetails(id) => {
                let detail = if fields.requested("detail") {
                    Some(
                        services
                            .catalog
                            .recording_metadata(actor, id, Default::default())
                            .await?,
                    )
                } else {
                    None
                };
                Ok(PageProps::RecordingDetails(Box::new(
                    RecordingDetailsPageProps { shared, detail },
                )))
            }
            Self::AlbumIndex => {
                let query = fields
                    .requested("query")
                    .then(|| search.q.clone().unwrap_or_default());
                let releases = if fields.requested("releases") {
                    Some(ReleasePage(
                        services.catalog.releases(actor, browse(search)).await?,
                    ))
                } else {
                    None
                };
                Ok(PageProps::AlbumIndex(AlbumIndexPageProps {
                    shared,
                    query,
                    releases,
                }))
            }
            Self::ArtistIndex => {
                let query = fields
                    .requested("query")
                    .then(|| search.q.clone().unwrap_or_default());
                let artists = if fields.requested("artists") {
                    Some(ArtistPage(
                        services.catalog.artists(actor, browse(search)).await?,
                    ))
                } else {
                    None
                };
                Ok(PageProps::ArtistIndex(ArtistIndexPageProps {
                    shared,
                    query,
                    artists,
                }))
            }
            Self::Album(id) => {
                let detail = if fields.requested("detail") {
                    Some(services.catalog.release(actor, id, browse(search)).await?)
                } else {
                    None
                };
                Ok(PageProps::Album(AlbumPageProps { shared, detail }))
            }
            Self::Artist(id) => {
                let detail = if fields.requested("detail") {
                    let mut browse = browse(search);
                    browse.recording_offset = pagination.recording_offset;
                    Some(services.catalog.artist(actor, id, browse).await?)
                } else {
                    None
                };
                Ok(PageProps::Artist(ArtistPageProps { shared, detail }))
            }
            Self::Catalog | Self::Likes => {
                if matches!(self, Self::Likes) {
                    search.liked = Some(true);
                }
                let query = fields
                    .requested("query")
                    .then(|| search.q.clone().unwrap_or_default());
                let recordings = if fields.requested("recordings") {
                    Some(RecordingPage(
                        services.catalog.recordings(actor, search).await?,
                    ))
                } else {
                    None
                };
                let props = CatalogPageProps {
                    shared,
                    query,
                    recordings,
                };
                Ok(if matches!(self, Self::Likes) {
                    PageProps::Likes(props)
                } else {
                    PageProps::Catalog(props)
                })
            }
            Self::AudiobookIndex => {
                let page = if fields.requested("books") || fields.requested("book_pagination") {
                    Some(
                        services
                            .collection
                            .audiobook_library(
                                actor,
                                AudiobookLibraryQuery {
                                    limit: search.limit.map(u16::try_from).transpose().map_err(
                                        |_| {
                                            Error::Invalid(
                                                "Audiobook pages contain 1 to 100 books".into(),
                                            )
                                        },
                                    )?,
                                    cursor: pagination.cursor,
                                },
                            )
                            .await?,
                    )
                } else {
                    None
                };
                let (books, book_pagination) = match page {
                    Some(page) => (
                        fields.requested("books").then_some(page.items),
                        fields
                            .requested("book_pagination")
                            .then_some(page.pagination),
                    ),
                    None => (None, None),
                };
                let grouping_playlists = if fields.requested("grouping_playlists") {
                    Some(
                        services
                            .collection
                            .audiobook_grouping_playlists(actor)
                            .await?,
                    )
                } else {
                    None
                };
                Ok(PageProps::AudiobookIndex(AudiobookIndexPageProps {
                    shared,
                    books,
                    book_pagination,
                    grouping_playlists,
                }))
            }
            Self::Audiobook(id) => {
                let detail = services.collection.audiobook(actor, id).await?;
                Ok(PageProps::Audiobook(AudiobookPageProps {
                    shared,
                    detail: fields.requested("detail").then_some(detail),
                }))
            }
            Self::PlaylistIndex => {
                let (connections, service_names) =
                    if fields.requested("connections") || fields.requested("service_names") {
                        let settings = services.plugins.settings(actor).await?;
                        let service_names = fields.requested("service_names").then(|| {
                            let connected: std::collections::HashSet<&str> = settings
                                .installations
                                .iter()
                                .map(|installation| installation.service_id.as_str())
                                .collect();
                            settings
                                .services
                                .into_iter()
                                .filter(|service| connected.contains(service.id.as_str()))
                                .map(|service| (service.id, service.name))
                                .collect()
                        });
                        (
                            fields
                                .requested("connections")
                                .then_some(settings.installations),
                            service_names,
                        )
                    } else {
                        (None, None)
                    };
                Ok(PageProps::PlaylistIndex(PlaylistIndexPageProps {
                    shared,
                    connections,
                    service_names,
                }))
            }
            Self::Playlist(id) => {
                let detail = if fields.requested("detail") {
                    Some(services.collection.playlist(actor, id).await?)
                } else {
                    None
                };
                Ok(PageProps::Playlist(PlaylistPageProps { shared, detail }))
            }
            Self::History => {
                let activity = if fields.requested("activity") {
                    Some(services.collection.activity(actor).await?)
                } else {
                    None
                };
                Ok(PageProps::History(HistoryPageProps { shared, activity }))
            }
            Self::Settings => Ok(PageProps::Settings(SettingsPageProps { shared })),
            Self::Discover => {
                let (connections, service_names) =
                    if fields.requested("connections") || fields.requested("service_names") {
                        let settings = services.plugins.settings(actor).await?;
                        (
                            fields
                                .requested("connections")
                                .then_some(settings.installations),
                            fields.requested("service_names").then(|| {
                                settings
                                    .services
                                    .into_iter()
                                    .map(|service| (service.id, service.name))
                                    .collect()
                            }),
                        )
                    } else {
                        (None, None)
                    };
                Ok(PageProps::Discover(DiscoverPageProps {
                    shared,
                    connections,
                    service_names,
                }))
            }
        }
    }
}

fn browse(search: Search) -> Browse {
    Browse {
        q: search.q,
        library: search.library,
        offset: search.offset,
        limit: search.limit,
        ..Browse::default()
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
    Ok(SharedPageProps {
        errors: Default::default(),
        native_playback: None,
        user: fields.requested("user").then(|| actor.user().clone()),
        libraries,
        playlists,
    })
}
