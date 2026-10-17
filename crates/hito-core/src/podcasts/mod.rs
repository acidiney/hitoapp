mod episodes;
mod feed;
mod refresh;
mod subscriptions;
mod types;

use crate::{Catalog, remote_http::RemoteHttp, store::Store};
pub use types::{
    PodcastEpisode, PodcastEpisodePage, PodcastEpisodeQuery, PodcastRefreshReceipt,
    PodcastSubscription, PodcastSubscriptionReceipt, RefreshPodcast, SubscribePodcast,
};

#[derive(Clone)]
pub struct Podcasts {
    pub(crate) store: Store,
    pub(crate) catalog: Catalog,
    pub(crate) remote: RemoteHttp,
}
