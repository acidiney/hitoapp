import type { Schema } from "../../../shared/schema";
import { requestJson } from "../../../shared/http";

export const sourceBindingsApi = {
  async review(
    account: string,
    recording: string,
    body: Schema<"ProviderCandidate">,
    signal: AbortSignal,
  ): Promise<Schema<"ProviderSourceReview">> {
    return requestJson<Schema<"ProviderSourceReview">>(
      `/recordings/${encodeURIComponent(recording)}/sources/review`,
      {
        action: "sources.review",
        method: "POST",
        account,
        body,
        signal,
      },
    );
  },
  async attach(
    account: string,
    recording: string,
    body: Schema<"AttachReviewedProviderSource">,
    signal: AbortSignal,
  ): Promise<Schema<"Recording">> {
    return requestJson<Schema<"Recording">>(
      `/recordings/${encodeURIComponent(recording)}/sources/attach`,
      {
        action: "sources.attach",
        method: "POST",
        account,
        body,
        signal,
      },
    );
  },
};
