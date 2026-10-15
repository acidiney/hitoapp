use super::{props::*, protocol::Protocol};
use crate::api::browsing::{ArtistPage, ReleasePage};
use crate::{api::catalog::RecordingPage, error::ApiResult};
use hito_core::Browse;
use hito_core::{Actor, Search, Services};

pub enum Screen<'a> {
    Catalog,
    AlbumIndex,
    Album(&'a str),
    ArtistIndex,
    Artist(&'a str),
    Likes,
    PlaylistIndex,
    Playlist(&'a str),
    History,
    Settings,
}

impl<'a> Screen<'a> {
    pub fn from_path(path: &'a str) -> Self {
        match path {
            "/" | "/library" => Self::Catalog,
            "/likes" => Self::Likes,
            "/playlists" => Self::PlaylistIndex,
            "/history" => Self::History,
            "/settings" => Self::Settings,
            "/albums" => Self::AlbumIndex,
            "/artists" => Self::ArtistIndex,
            _ if path.starts_with("/albums/") => Self::Album(path.trim_start_matches("/albums/")),
            _ if path.starts_with("/artists/") => {
                Self::Artist(path.trim_start_matches("/artists/"))
            }
            _ => Self::Playlist(path.trim_start_matches("/playlists/")),
        }
    }

    pub fn component(&self) -> &'static str {
        match self {
            Self::Catalog => "CatalogPage",
            Self::AlbumIndex => "AlbumIndexPage",
            Self::Album(_) => "AlbumPage",
            Self::ArtistIndex => "ArtistIndexPage",
            Self::Artist(_) => "ArtistPage",
            Self::Likes => "LikesPage",
            Self::PlaylistIndex => "PlaylistIndexPage",
            Self::Playlist(_) => "PlaylistPage",
            Self::History => "HistoryPage",
            Self::Settings => "SettingsPage",
        }
    }

    pub async fn props(
        &self,
        services: &Services,
        actor: &Actor,
        mut search: Search,
        fields: &Protocol,
    ) -> ApiResult<PageProps> {
        let shared = shared(services, actor, fields).await?;
        match self {
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
                    Some(services.catalog.artist(actor, id, browse(search)).await?)
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
            Self::PlaylistIndex => Ok(PageProps::PlaylistIndex(PlaylistIndexPageProps { shared })),
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
        user: fields.requested("user").then(|| actor.user().clone()),
        libraries,
        playlists,
    })
}
