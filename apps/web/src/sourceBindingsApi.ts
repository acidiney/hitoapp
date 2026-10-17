import { request } from "./http";
import type {
  AttachReviewedProviderSource,
  ProviderCandidate,
  ProviderSourceReview,
  Recording,
} from "./types";

export const sourceBindingsApi = {
  async review(
    account: string,
    recording: string,
    body: ProviderCandidate,
    signal: AbortSignal,
  ): Promise<ProviderSourceReview> {
    return (
      await request(
        `/recordings/${encodeURIComponent(recording)}/sources/review`,
        {
          action: "sources.review",
          method: "POST",
          account,
          body,
          signal,
        },
      )
    ).json();
  },
  async attach(
    account: string,
    recording: string,
    body: AttachReviewedProviderSource,
    signal: AbortSignal,
  ): Promise<Recording> {
    return (
      await request(
        `/recordings/${encodeURIComponent(recording)}/sources/attach`,
        {
          action: "sources.attach",
          method: "POST",
          account,
          body,
          signal,
        },
      )
    ).json();
  },
};
