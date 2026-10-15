use crate::api::catalog::RecordingPage;
use hito_core::{Activity, Library, Playlist, PlaylistDetail, User};
use serde::Serialize;
use std::collections::BTreeMap;
use utoipa::ToSchema;

#[derive(Serialize, ToSchema)]
pub struct SharedPageProps {
    pub errors: BTreeMap<String, String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schema(required = true, nullable = false)]
    pub user: Option<User>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schema(required = true, nullable = false)]
    pub libraries: Option<Vec<Library>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schema(required = true, nullable = false)]
    pub playlists: Option<Vec<Playlist>>,
}

#[derive(Serialize, ToSchema)]
pub struct LoginPageProps {
    pub errors: BTreeMap<String, String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schema(required = true, nullable = false)]
    pub needs_setup: Option<bool>,
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
pub struct PlaylistIndexPageProps {
    #[serde(flatten)]
    pub shared: SharedPageProps,
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

#[derive(Serialize)]
#[serde(untagged)]
pub enum PageProps {
    Login(LoginPageProps),
    Catalog(CatalogPageProps),
    Likes(CatalogPageProps),
    PlaylistIndex(PlaylistIndexPageProps),
    Playlist(PlaylistPageProps),
    History(HistoryPageProps),
    Settings(SettingsPageProps),
}

impl PageProps {
    pub fn component(&self) -> &'static str {
        match self {
            Self::Login(_) => "LoginPage",
            Self::Catalog(_) => "CatalogPage",
            Self::Likes(_) => "LikesPage",
            Self::PlaylistIndex(_) => "PlaylistIndexPage",
            Self::Playlist(_) => "PlaylistPage",
            Self::History(_) => "HistoryPage",
            Self::Settings(_) => "SettingsPage",
        }
    }
}
