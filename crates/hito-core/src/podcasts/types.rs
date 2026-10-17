use crate::Recording;
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use uuid::Uuid;

#[derive(Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct SubscribePodcast {
    pub request_id: Uuid,
    pub feed_url: String,
}

#[derive(Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct RefreshPodcast {
    pub request_id: Uuid,
}

#[derive(Serialize, ToSchema)]
pub struct PodcastRefreshReceipt {
    pub request_id: Uuid,
    pub subscription_id: Uuid,
    pub job_id: Uuid,
}

#[derive(Serialize, ToSchema)]
pub struct PodcastSubscriptionReceipt {
    pub request_id: Uuid,
    pub subscription_id: Uuid,
    pub job_id: Uuid,
}

#[derive(Serialize, ToSchema)]
pub struct PodcastSubscription {
    pub subscription_id: Uuid,
    pub title: Option<String>,
}

#[derive(Serialize, ToSchema)]
pub struct PodcastEpisode {
    pub episode_id: Uuid,
    pub recording: Recording,
}

#[derive(Default, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct PodcastEpisodeQuery {
    pub limit: Option<u16>,
    pub after: Option<String>,
}

#[derive(Serialize, ToSchema)]
pub struct PodcastEpisodePage {
    pub items: Vec<PodcastEpisode>,
    pub total: i64,
    pub limit: u16,
    pub next_cursor: Option<String>,
}
