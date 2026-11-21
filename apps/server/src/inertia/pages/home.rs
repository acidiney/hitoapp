use super::PageRequest;
use crate::api::browsing::ArtistPage;
use crate::error::ApiResult;
use crate::inertia::props::*;
use hito_core::Browse;

const HOME_ARTISTS: i64 = 20;

pub(super) async fn home(request: PageRequest<'_>) -> ApiResult<PageProps> {
    let home = request
        .when("home", async {
            Ok(request.services.collection.home(request.actor).await?)
        })
        .await?;
    let artists = request
        .when("artists", async {
            Ok(ArtistPage(
                request
                    .services
                    .catalog
                    .artists(
                        request.actor,
                        Browse {
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
