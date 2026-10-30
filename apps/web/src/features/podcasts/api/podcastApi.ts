import type { Schema } from "../../../shared/schema";
import { requestJson } from "../../../shared/http";

export const podcastApi = {
  async unsubscribe(
    account: string,
    subscription: string,
    body: Readonly<Schema<"UnsubscribePodcast">>,
    signal: AbortSignal,
  ): Promise<Schema<"PodcastUnsubscribeReceipt">> {
    return requestJson<Schema<"PodcastUnsubscribeReceipt">>(
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
    body: Readonly<Schema<"RefreshPodcast">>,
    signal: AbortSignal,
  ): Promise<Schema<"PodcastRefreshReceipt">> {
    return requestJson<Schema<"PodcastRefreshReceipt">>(
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
  ): Promise<Schema<"PodcastEpisodePage">> {
    const query = new URLSearchParams({ limit: String(limit) });
    if (after !== null) query.set("after", after);
    return requestJson<Schema<"PodcastEpisodePage">>(
      `/podcasts/${encodeURIComponent(subscription)}/episodes?${query}`,
      { action: "podcasts.episodes", account, signal },
    );
  },
  async progress(
    account: string,
    recordings: string[],
    signal: AbortSignal,
  ): Promise<Schema<"RecordingProgress">[]> {
    return requestJson<Schema<"RecordingProgress">[]>("/recordings/progress", {
      action: "podcasts.progress",
      method: "POST",
      account,
      signal,
      body: {
        recording_ids: recordings,
      } satisfies Schema<"SavedPlaybackRequest">,
    });
  },
  async subscriptions(
    account: string,
    signal: AbortSignal,
  ): Promise<Schema<"PodcastSubscription">[]> {
    return requestJson<Schema<"PodcastSubscription">[]>("/podcasts", {
      action: "podcasts.status",
      account,
      signal,
    });
  },
  async subscribe(
    account: string,
    body: Readonly<Schema<"SubscribePodcast">>,
    signal: AbortSignal,
  ): Promise<Schema<"PodcastSubscriptionReceipt">> {
    return requestJson<Schema<"PodcastSubscriptionReceipt">>("/podcasts", {
      action: "podcasts.subscribe",
      method: "POST",
      account,
      body,
      signal,
    });
  },
};
