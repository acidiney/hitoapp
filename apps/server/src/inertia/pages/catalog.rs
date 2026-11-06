use super::PageRequest;
use crate::api::artist_identity_groups::ArtistIdentityGroupPage;
use crate::api::browsing::{ArtistPage, ReleasePage};
use crate::inertia::props::*;
use crate::{api::catalog::RecordingPage, error::ApiResult};
use hito_core::{ArtistIdentityGroupBrowse, ArtistIdentityGroupQuery, Browse, Search};

pub(super) async fn recordings(request: PageRequest<'_>) -> ApiResult<PageProps> {
    let props = recording_list(request, false).await?;
    Ok(PageProps::Catalog(props))
}

pub(super) async fn likes(request: PageRequest<'_>) -> ApiResult<PageProps> {
    let props = recording_list(request, true).await?;
    Ok(PageProps::Likes(props))
}

async fn recording_list(request: PageRequest<'_>, liked: bool) -> ApiResult<CatalogPageProps> {
    let query = request.query();
    let catalog = &request.services.catalog;
    let recordings = if request.requested("recordings") {
        let page = if liked {
            catalog
                .liked_recordings(request.actor, request.search)
                .await?
        } else {
            catalog.recordings(request.actor, request.search).await?
        };
        Some(RecordingPage(page))
    } else {
        None
    };
    Ok(CatalogPageProps {
        shared: request.shared,
        query,
        recordings,
    })
}

pub(super) async fn recording_details(request: PageRequest<'_>, id: &str) -> ApiResult<PageProps> {
    let detail = request
        .when("detail", async {
            Ok(request
                .services
                .catalog
                .recording_metadata(request.actor, id, Default::default())
                .await?)
        })
        .await?;
    let copies = request
        .when("copies", async {
            Ok(request
                .services
                .catalog
                .recording_copies(request.actor, id)
                .await?)
        })
        .await?;
    Ok(PageProps::RecordingDetails(Box::new(
        RecordingDetailsPageProps {
            shared: request.shared,
            detail,
            copies,
        },
    )))
}

pub(super) async fn album_index(request: PageRequest<'_>) -> ApiResult<PageProps> {
    let query = request.query();
    let releases = request
        .when("releases", async {
            Ok(ReleasePage(
                request
                    .services
                    .catalog
                    .releases(request.actor, browse(request.search.clone()))
                    .await?,
            ))
        })
        .await?;
    Ok(PageProps::AlbumIndex(AlbumIndexPageProps {
        shared: request.shared,
        query,
        releases,
    }))
}

pub(super) async fn album(request: PageRequest<'_>, id: &str) -> ApiResult<PageProps> {
    let detail = request
        .when("detail", async {
            Ok(request
                .services
                .artist_catalog
                .release(request.actor, id, browse(request.search.clone()))
                .await?)
        })
        .await?;
    Ok(PageProps::Album(AlbumPageProps {
        shared: request.shared,
        detail,
    }))
}

pub(super) async fn album_details(request: PageRequest<'_>, id: &str) -> ApiResult<PageProps> {
    let detail = request
        .when("detail", async {
            Ok(request
                .services
                .catalog
                .release_metadata(request.actor, id, Default::default())
                .await?)
        })
        .await?;
    Ok(PageProps::AlbumDetails(AlbumDetailsPageProps {
        shared: request.shared,
        detail,
    }))
}

pub(super) async fn artist_index(request: PageRequest<'_>) -> ApiResult<PageProps> {
    let catalog = &request.services.catalog;
    let artist_catalog = &request.services.artist_catalog;
    let query = request.query();
    let identity_groups = request
        .when("identity_groups", async {
            Ok(ArtistIdentityGroupPage(
                artist_catalog
                    .artist_identity_groups(
                        request.actor,
                        ArtistIdentityGroupQuery {
                            q: request.search.q.clone(),
                            library: request.search.library.clone(),
                            offset: request.pagination.group_offset,
                            limit: request.search.limit,
                        },
                    )
                    .await?,
            ))
        })
        .await?;
    let artists = request
        .when("artists", async {
            Ok(ArtistPage(
                catalog
                    .artists(request.actor, browse(request.search.clone()))
                    .await?,
            ))
        })
        .await?;
    Ok(PageProps::ArtistIndex(ArtistIndexPageProps {
        shared: request.shared,
        query,
        artists,
        identity_groups,
    }))
}

pub(super) async fn artist(request: PageRequest<'_>, id: &str) -> ApiResult<PageProps> {
    let detail = request
        .when("detail", async {
            let mut browse = browse(request.search.clone());
            browse.recording_offset = request.pagination.recording_offset;
            Ok(request
                .services
                .catalog
                .artist(request.actor, id, browse)
                .await?)
        })
        .await?;
    Ok(PageProps::Artist(ArtistPageProps {
        shared: request.shared,
        detail,
    }))
}

pub(super) async fn catalog_album(request: PageRequest<'_>, id: &str) -> ApiResult<PageProps> {
    let detail = request
        .when("detail", async {
            Ok(request
                .services
                .artist_catalog
                .artist_identity_group(
                    request.actor,
                    id,
                    ArtistIdentityGroupBrowse {
                        library: None,
                        library_offset: None,
                        recording_offset: None,
                        release_offset: None,
                        limit: Some(1),
                    },
                )
                .await?)
        })
        .await?;
    Ok(PageProps::CatalogAlbum(ArtistIdentityGroupPageProps {
        shared: request.shared,
        detail,
    }))
}

pub(super) async fn artist_identity_group(
    request: PageRequest<'_>,
    id: &str,
) -> ApiResult<PageProps> {
    let detail = request
        .when("detail", async {
            Ok(request
                .services
                .artist_catalog
                .artist_identity_group(
                    request.actor,
                    id,
                    ArtistIdentityGroupBrowse {
                        library: request.search.library.clone(),
                        library_offset: request.pagination.library_offset,
                        recording_offset: request.pagination.recording_offset,
                        release_offset: request.pagination.release_offset,
                        limit: request.search.limit,
                    },
                )
                .await?)
        })
        .await?;
    Ok(PageProps::ArtistIdentityGroup(
        ArtistIdentityGroupPageProps {
            shared: request.shared,
            detail,
        },
    ))
}

pub(super) fn browse(search: Search) -> Browse {
    Browse {
        q: search.q,
        library: search.library,
        offset: search.offset,
        limit: search.limit,
        ..Browse::default()
    }
}
