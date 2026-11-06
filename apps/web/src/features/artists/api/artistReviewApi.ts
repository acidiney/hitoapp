import { cachedRead } from "./catalogCache";
import type { Schema } from "../../../shared/schema";
import { requestJson } from "../../../shared/http";

const part = encodeURIComponent;

export const artistReviewApi = {
  async context(
    account: string,
    recording: string,
    body: Schema<"BeginArtistReview">,
    signal: AbortSignal,
  ): Promise<Schema<"ArtistReviewContext">> {
    return requestJson<Schema<"ArtistReviewContext">>(
      `/recordings/${part(recording)}/artist-review/context`,
      {
        action: "artist_review.context",
        method: "POST",
        account,
        body,
        signal,
      },
    );
  },
  async search(
    account: string,
    recording: string,
    body: Schema<"SearchArtistCandidates">,
    signal: AbortSignal,
  ): Promise<Schema<"ArtistCandidateSearchAvailability">> {
    return requestJson<Schema<"ArtistCandidateSearchAvailability">>(
      `/recordings/${part(recording)}/artist-review/search`,
      {
        action: "artist_review.search",
        method: "POST",
        account,
        body,
        signal,
      },
    );
  },
  async preview(
    account: string,
    recording: string,
    body: Schema<"SelectArtistCandidate">,
    signal: AbortSignal,
  ): Promise<Schema<"ArtistCandidatePreviewAvailability">> {
    return requestJson<Schema<"ArtistCandidatePreviewAvailability">>(
      `/recordings/${part(recording)}/artist-review/preview`,
      {
        action: "artist_review.preview",
        method: "POST",
        account,
        body,
        signal,
      },
    );
  },
  async releases(
    account: string,
    recording: string,
    body: Schema<"SelectArtistCandidate">,
    signal: AbortSignal,
  ): Promise<Schema<"ArtistCandidateReleasesAvailability">> {
    return requestJson<Schema<"ArtistCandidateReleasesAvailability">>(
      `/recordings/${part(recording)}/artist-review/releases`,
      {
        action: "artist_review.releases",
        method: "POST",
        account,
        body,
        signal,
      },
    );
  },
  async confirm(
    account: string,
    recording: string,
    body: Schema<"SelectArtistCandidate">,
    signal: AbortSignal,
  ): Promise<Schema<"ArtistAssociation">> {
    return requestJson<Schema<"ArtistAssociation">>(
      `/recordings/${part(recording)}/artist-review/confirm`,
      {
        action: "artist_review.confirm",
        method: "POST",
        account,
        body,
        signal,
      },
    );
  },
  async associations(
    account: string,
    recording: string,
    signal: AbortSignal,
  ): Promise<Schema<"ArtistAssociation">[]> {
    return requestJson<Schema<"ArtistAssociation">[]>(
      `/recordings/${part(recording)}/artist-associations`,
      {
        action: "artist_review.associations",
        account,
        signal,
      },
    );
  },
  async profile(
    account: string,
    recording: string,
    association: string,
    signal: AbortSignal,
  ): Promise<Schema<"ArtistProfileAvailability">> {
    return cachedRead(
      account,
      ["profile", recording, association],
      (loadSignal) =>
        requestJson<Schema<"ArtistProfileAvailability">>(
          `/recordings/${part(recording)}/artist-associations/${part(association)}/profile`,
          {
            action: "artist_review.profile",
            account,
            signal: loadSignal,
          },
        ),
      signal,
    );
  },
};
