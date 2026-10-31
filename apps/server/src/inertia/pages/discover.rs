use super::PageRequest;
use crate::api::artist_identity_groups::ArtistIdentityGroupPage;
use crate::error::ApiResult;
use crate::inertia::props::*;
use hito_core::ArtistIdentityGroupQuery;

const RADIO_ARTISTS: i64 = 6;

pub(super) async fn discover(request: PageRequest<'_>) -> ApiResult<PageProps> {
    let (connections, service_names) =
        if request.requested("connections") || request.requested("service_names") {
            let offered = request.services.plugins.connections(request.actor).await?;
            let service_names = request.requested("service_names").then(|| {
                offered
                    .offered_services()
                    .map(|service| (service.id.clone(), service.name.clone()))
                    .collect()
            });
            (
                request
                    .requested("connections")
                    .then_some(offered.installations),
                service_names,
            )
        } else {
            (None, None)
        };
    let genres = request
        .when("genres", async {
            Ok(request.services.catalog.genres(request.actor).await?)
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
                            limit: Some(RADIO_ARTISTS),
                            ..Default::default()
                        },
                    )
                    .await?,
            ))
        })
        .await?;
    Ok(PageProps::Discover(DiscoverPageProps {
        shared: request.shared,
        connections,
        service_names,
        genres,
        artists,
    }))
}
