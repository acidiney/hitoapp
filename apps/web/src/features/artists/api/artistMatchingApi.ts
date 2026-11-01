import type { Schema } from "../../../shared/schema";
import { request } from "../../../shared/http";

const part = encodeURIComponent;

async function json<T>(
  path: string,
  options: {
    action: string;
    account: string;
    signal: AbortSignal;
    method?: "GET" | "POST";
    body?:
      | Schema<"MatchArtistsFromProviderEvidence">
      | Schema<"DismissArtistMatchSuggestion">
      | Schema<"RestoreArtistMatchSuggestion">
      | Schema<"ConfirmArtistReviewCandidate">
      | Schema<"DismissArtistReviewCandidates">
      | Schema<"RestoreArtistReviewCandidates">;
  },
): Promise<T> {
  const response = await request(path, options);
  if (options.signal.aborted) {
    await response.body?.cancel();
    options.signal.throwIfAborted();
  }
  return response.json();
}

export const artistMatchingApi = {
  status(account: string, signal: AbortSignal) {
    return json<Schema<"ArtistMatchingStatus">>("/artist-matching/status", {
      action: "artist_matching.status",
      account,
      signal,
    });
  },
  runProviderEvidence(
    account: string,
    body: Schema<"MatchArtistsFromProviderEvidence">,
    signal: AbortSignal,
  ) {
    return json<Schema<"ArtistProviderMatchReceipt">>(
      "/artist-matching/provider-evidence",
      {
        action: "artist_matching.run_provider_evidence",
        account,
        signal,
        method: "POST",
        body,
      },
    );
  },
  suggestions(account: string, artist: string, signal: AbortSignal) {
    return json<Schema<"ArtistMatchSuggestions">>(
      `/artists/${part(artist)}/match-suggestions`,
      { action: "artist_matching.suggestions", account, signal },
    );
  },
  dismiss(
    account: string,
    artist: string,
    body: Schema<"DismissArtistMatchSuggestion">,
    signal: AbortSignal,
  ) {
    return json<Schema<"ArtistMatchSuggestions">>(
      `/artists/${part(artist)}/match-suggestions/dismiss`,
      {
        action: "artist_matching.dismiss",
        account,
        signal,
        method: "POST",
        body,
      },
    );
  },
  restore(
    account: string,
    artist: string,
    body: Schema<"RestoreArtistMatchSuggestion">,
    signal: AbortSignal,
  ) {
    return json<Schema<"ArtistMatchSuggestions">>(
      `/artists/${part(artist)}/match-suggestions/restore`,
      {
        action: "artist_matching.restore",
        account,
        signal,
        method: "POST",
        body,
      },
    );
  },
  reviewQueue(account: string, signal: AbortSignal) {
    return json<Schema<"ArtistReviewQueuePage">>(
      "/artist-matching/review-queue?limit=50",
      { action: "artist_matching.review_queue", account, signal },
    );
  },
  confirmCandidate(
    account: string,
    credit: string,
    body: Schema<"ConfirmArtistReviewCandidate">,
    signal: AbortSignal,
  ) {
    return json<Schema<"ArtistReviewConfirmation">>(
      `/artist-matching/review-queue/${part(credit)}/confirm`,
      {
        action: "artist_matching.review_confirm",
        account,
        signal,
        method: "POST",
        body,
      },
    );
  },
  dismissCandidates(
    account: string,
    credit: string,
    body: Schema<"DismissArtistReviewCandidates">,
    signal: AbortSignal,
  ) {
    return json<Schema<"ArtistReviewDismissal">>(
      `/artist-matching/review-queue/${part(credit)}/dismiss`,
      {
        action: "artist_matching.review_dismiss",
        account,
        signal,
        method: "POST",
        body,
      },
    );
  },
  dismissedReviews(account: string, signal: AbortSignal) {
    return json<Schema<"ArtistReviewQueuePage">>(
      "/artist-matching/review-queue/dismissed?limit=50",
      { action: "artist_matching.review_dismissed", account, signal },
    );
  },
  restoreCandidates(
    account: string,
    credit: string,
    body: Schema<"RestoreArtistReviewCandidates">,
    signal: AbortSignal,
  ) {
    return json<Schema<"ArtistReviewRestoration">>(
      `/artist-matching/review-queue/${part(credit)}/restore`,
      {
        action: "artist_matching.review_restore",
        account,
        signal,
        method: "POST",
        body,
      },
    );
  },
};
