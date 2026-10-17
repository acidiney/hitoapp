mod episodes;
mod feed;
mod lifecycle;
mod refresh;
mod subscriptions;
mod types;

#[cfg(test)]
#[path = "tests/archive_query_work.rs"]
mod archive_query_work;
#[cfg(test)]
#[path = "tests/query_work.rs"]
mod query_work;
#[cfg(test)]
#[path = "tests/work_meter.rs"]
mod work_meter;

use crate::{Catalog, remote_http::RemoteHttp, store::Store};
pub use types::{
    PodcastEpisode, PodcastEpisodePage, PodcastEpisodeQuery, PodcastRefreshReceipt,
    PodcastSubscription, PodcastSubscriptionReceipt, PodcastUnsubscribeReceipt, RefreshPodcast,
    SubscribePodcast, UnsubscribePodcast,
};

#[derive(Clone)]
pub struct Podcasts {
    pub(crate) store: Store,
    pub(crate) catalog: Catalog,
    pub(crate) remote: RemoteHttp,
}
