import { request } from "../../../shared/http";
import type {
  PodcastEpisodePage,
  PodcastRefreshReceipt,
  PodcastUnsubscribeReceipt,
  PodcastSubscription,
  PodcastSubscriptionReceipt,
  RecordingProgress,
  RefreshPodcast,
  SavedPlaybackRequest,
  SubscribePodcast,
  UnsubscribePodcast,
} from "../../../shared/types";

export const podcastApi = {
  async unsubscribe(
    account: string,
    subscription: string,
    body: Readonly<UnsubscribePodcast>,
    signal: AbortSignal,
  ): Promise<PodcastUnsubscribeReceipt> {
    return (
      await request(
        `/podcasts/${encodeURIComponent(subscription)}/unsubscribe`,
        {
          action: "podcasts.unsubscribe",
          method: "POST",
          account,
          body,
          signal,
        },
      )
    ).json();
  },
  async refresh(
    account: string,
    subscription: string,
    body: Readonly<RefreshPodcast>,
    signal: AbortSignal,
  ): Promise<PodcastRefreshReceipt> {
    return (
      await request(`/podcasts/${encodeURIComponent(subscription)}/refresh`, {
        action: "podcasts.refresh",
        method: "POST",
        account,
        body,
        signal,
      })
    ).json();
  },
  async episodes(
    account: string,
    subscription: string,
    limit: number,
    after: string | null,
    signal: AbortSignal,
  ): Promise<PodcastEpisodePage> {
    const query = new URLSearchParams({ limit: String(limit) });
    if (after !== null) query.set("after", after);
    return (
      await request(
        `/podcasts/${encodeURIComponent(subscription)}/episodes?${query}`,
        { action: "podcasts.episodes", account, signal },
      )
    ).json();
  },
  async progress(
    account: string,
    recordings: string[],
    signal: AbortSignal,
  ): Promise<RecordingProgress[]> {
    return (
      await request("/recordings/progress", {
        action: "podcasts.progress",
        method: "POST",
        account,
        signal,
        body: { recording_ids: recordings } satisfies SavedPlaybackRequest,
      })
    ).json();
  },
  async subscriptions(
    account: string,
    signal: AbortSignal,
  ): Promise<PodcastSubscription[]> {
    return (
      await request("/podcasts", {
        action: "podcasts.status",
        account,
        signal,
      })
    ).json();
  },
  async subscribe(
    account: string,
    body: Readonly<SubscribePodcast>,
    signal: AbortSignal,
  ): Promise<PodcastSubscriptionReceipt> {
    return (
      await request("/podcasts", {
        action: "podcasts.subscribe",
        method: "POST",
        account,
        body,
        signal,
      })
    ).json();
  },
};
