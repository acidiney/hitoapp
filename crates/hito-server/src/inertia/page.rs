use super::{props::*, protocol::Protocol};
use crate::{api::catalog::RecordingPage, error::ApiResult};
use hito_core::{Actor, Search, Services};

pub enum Screen<'a> {
    Catalog,
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
            _ => Self::Playlist(path.trim_start_matches("/playlists/")),
        }
    }

    pub fn component(&self) -> &'static str {
        match self {
            Self::Catalog => "CatalogPage",
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
