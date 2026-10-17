import { request } from "./http";
import type {
  PodcastSubscription,
  PodcastSubscriptionReceipt,
  SubscribePodcast,
} from "./types";

export const podcastApi = {
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
