use super::{PageRequest, catalog::browse};
use crate::api::browsing::{ArtistPage, ReleasePage};
use crate::api::catalog::RecordingPage;
use crate::error::ApiResult;
use crate::inertia::props::*;
use hito_core::{Page, Search};

const TRACKS: i64 = 8;
const ALBUMS_AND_ARTISTS: i64 = 6;

fn nothing<T>(limit: i64) -> Page<T> {
    Page {
        items: Vec::new(),
        total: 0,
        offset: 0,
        limit,
    }
}

pub(super) async fn search(request: PageRequest<'_>) -> ApiResult<PageProps> {
    let query = request.query();
    let catalog = &request.services.catalog;
    let text = request.search.q.clone().unwrap_or_default();
    let searching = !text.trim().is_empty();
    let bounded = |limit: i64| Search {
        offset: Some(0),
        limit: Some(limit),
        library: None,
        artist: None,
        liked: None,
        ..request.search.clone()
    };
    let recordings = request
        .when("recordings", async {
            Ok(RecordingPage(if searching {
                catalog.recordings(request.actor, bounded(TRACKS)).await?
            } else {
                nothing(TRACKS)
            }))
        })
        .await?;
    let releases = request
        .when("releases", async {
            Ok(ReleasePage(if searching {
                catalog
                    .releases(request.actor, browse(bounded(ALBUMS_AND_ARTISTS)))
                    .await?
            } else {
                nothing(ALBUMS_AND_ARTISTS)
            }))
        })
        .await?;
    let artists = request
        .when("artists", async {
            Ok(ArtistPage(if searching {
                catalog
                    .artists(request.actor, browse(bounded(ALBUMS_AND_ARTISTS)))
                    .await?
            } else {
                nothing(ALBUMS_AND_ARTISTS)
            }))
        })
        .await?;
    let (connections, service_names) =
        if request.requested("connections") || request.requested("service_names") {
            let offered = request.services.plugins.connections(request.actor).await?;
            let names = request.requested("service_names").then(|| {
                offered
                    .offered_services()
                    .map(|service| (service.id.clone(), service.name.clone()))
                    .collect()
            });
            (
                request
                    .requested("connections")
                    .then_some(offered.installations),
                names,
            )
        } else {
            (None, None)
        };
    Ok(PageProps::Search(SearchPageProps {
        shared: request.shared,
        query,
        recordings,
        releases,
        artists,
        connections,
        service_names,
    }))
}
