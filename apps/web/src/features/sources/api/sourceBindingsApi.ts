import { requestJson } from "../../../shared/http";
import type {
  AttachReviewedProviderSource,
  ProviderCandidate,
  ProviderSourceReview,
  Recording,
} from "../../../shared/types";

export const sourceBindingsApi = {
  async review(
    account: string,
    recording: string,
    body: ProviderCandidate,
    signal: AbortSignal,
  ): Promise<ProviderSourceReview> {
    return requestJson<ProviderSourceReview>(
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
    body: AttachReviewedProviderSource,
    signal: AbortSignal,
  ): Promise<Recording> {
    return requestJson<Recording>(
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
