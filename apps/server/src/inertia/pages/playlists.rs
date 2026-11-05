use super::PageRequest;
use crate::error::ApiResult;
use crate::inertia::props::*;

pub(super) async fn index(request: PageRequest<'_>) -> ApiResult<PageProps> {
    let (connections, service_names) =
        if request.requested("connections") || request.requested("service_names") {
            let connected = request.services.plugins.connections(request.actor).await?;
            let service_names = request.requested("service_names").then(|| {
                connected
                    .connected_services()
                    .map(|service| (service.id.clone(), service.name.clone()))
                    .collect()
            });
            (
                request
                    .requested("connections")
                    .then_some(connected.installations),
                service_names,
            )
        } else {
            (None, None)
        };
    Ok(PageProps::PlaylistIndex(PlaylistIndexPageProps {
        shared: request.shared,
        connections,
        service_names,
    }))
}

pub(super) async fn detail(request: PageRequest<'_>, id: &str) -> ApiResult<PageProps> {
    let detail = request
        .when("detail", async {
            Ok(request
                .services
                .collection
                .playlist(request.actor, id)
                .await?)
        })
        .await?;
    Ok(PageProps::Playlist(PlaylistPageProps {
        shared: request.shared,
        detail,
    }))
}

pub(super) async fn history(request: PageRequest<'_>) -> ApiResult<PageProps> {
    let activity = request
        .when("activity", async {
            Ok(request.services.collection.activity(request.actor).await?)
        })
        .await?;
    let history_enabled = request
        .when("history_enabled", async {
            Ok(request
                .services
                .collection
                .history_preference(request.actor)
                .await?
                .enabled)
        })
        .await?;
    Ok(PageProps::History(HistoryPageProps {
        shared: request.shared,
        activity,
        history_enabled,
    }))
}
