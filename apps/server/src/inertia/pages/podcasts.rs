use super::PageRequest;
use crate::error::ApiResult;
use crate::inertia::props::*;
use hito_core::PodcastEpisodeQuery;

pub(super) async fn index(request: PageRequest<'_>) -> ApiResult<PageProps> {
    let subscriptions = request
        .when("subscriptions", async {
            Ok(request
                .services
                .podcasts
                .subscriptions(request.actor)
                .await?)
        })
        .await?;
    Ok(PageProps::PodcastIndex(PodcastIndexPageProps {
        shared: request.shared,
        subscriptions,
    }))
}

pub(super) async fn detail(request: PageRequest<'_>, id: &str) -> ApiResult<PageProps> {
    let podcasts = &request.services.podcasts;
    let subscription = podcasts.subscription(request.actor, id).await?;
    let query = PodcastEpisodeQuery::default();
    let (episodes, progress) = if request.requested("progress") {
        let joined = podcasts
            .episodes_with_progress(request.actor, id, query)
            .await?;
        (Some(joined.episodes), Some(joined.progress))
    } else if request.requested("episodes") {
        (
            Some(podcasts.episodes(request.actor, id, query).await?),
            None,
        )
    } else {
        (None, None)
    };
    Ok(PageProps::Podcast(PodcastPageProps {
        subscription: request.requested("subscription").then_some(subscription),
        episodes: episodes.filter(|_| request.requested("episodes")),
        progress,
        shared: request.shared,
    }))
}
