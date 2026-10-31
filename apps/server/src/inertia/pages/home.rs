use super::PageRequest;
use crate::api::artist_identity_groups::ArtistIdentityGroupPage;
use crate::error::ApiResult;
use crate::inertia::props::*;
use hito_core::ArtistIdentityGroupQuery;

const HOME_ARTISTS: i64 = 12;

pub(super) async fn home(request: PageRequest<'_>) -> ApiResult<PageProps> {
    let home = request
        .when("home", async {
            Ok(request.services.collection.home(request.actor).await?)
        })
        .await?;
    let artists = request
        .when("artists", async {
            Ok(ArtistIdentityGroupPage(
                request
                    .services
                    .artist_catalog
                    .artist_identity_groups(
                        request.actor,
                        ArtistIdentityGroupQuery {
                            limit: Some(HOME_ARTISTS),
                            ..Default::default()
                        },
                    )
                    .await?,
            ))
        })
        .await?;
    Ok(PageProps::Home(HomePageProps {
        shared: request.shared,
        home,
        artists,
    }))
}
