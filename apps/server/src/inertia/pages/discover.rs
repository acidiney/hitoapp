use super::PageRequest;
use crate::error::ApiResult;
use crate::inertia::props::*;

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
    Ok(PageProps::Discover(DiscoverPageProps {
        shared: request.shared,
        connections,
        service_names,
    }))
}
