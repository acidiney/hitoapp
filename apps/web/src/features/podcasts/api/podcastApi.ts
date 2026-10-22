import { requestJson } from "../../../shared/http";
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
    return requestJson<PodcastUnsubscribeReceipt>(
      `/podcasts/${encodeURIComponent(subscription)}/unsubscribe`,
      {
        action: "podcasts.unsubscribe",
        method: "POST",
        account,
        body,
        signal,
      },
    );
  },
  async refresh(
    account: string,
    subscription: string,
    body: Readonly<RefreshPodcast>,
    signal: AbortSignal,
  ): Promise<PodcastRefreshReceipt> {
    return requestJson<PodcastRefreshReceipt>(
      `/podcasts/${encodeURIComponent(subscription)}/refresh`,
      {
        action: "podcasts.refresh",
        method: "POST",
        account,
        body,
        signal,
      },
    );
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
    return requestJson<PodcastEpisodePage>(
      `/podcasts/${encodeURIComponent(subscription)}/episodes?${query}`,
      { action: "podcasts.episodes", account, signal },
    );
  },
  async progress(
    account: string,
    recordings: string[],
    signal: AbortSignal,
  ): Promise<RecordingProgress[]> {
    return requestJson<RecordingProgress[]>("/recordings/progress", {
      action: "podcasts.progress",
      method: "POST",
      account,
      signal,
      body: { recording_ids: recordings } satisfies SavedPlaybackRequest,
    });
  },
  async subscriptions(
    account: string,
    signal: AbortSignal,
  ): Promise<PodcastSubscription[]> {
    return requestJson<PodcastSubscription[]>("/podcasts", {
      action: "podcasts.status",
      account,
      signal,
    });
  },
  async subscribe(
    account: string,
    body: Readonly<SubscribePodcast>,
    signal: AbortSignal,
  ): Promise<PodcastSubscriptionReceipt> {
    return requestJson<PodcastSubscriptionReceipt>("/podcasts", {
      action: "podcasts.subscribe",
      method: "POST",
      account,
      body,
      signal,
    });
  },
};
