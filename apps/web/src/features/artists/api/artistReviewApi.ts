import { requestJson } from "../../../shared/http";
import type {
  ArtistAssociation,
  ArtistCandidatePreviewAvailability,
  ArtistCandidateSearchAvailability,
  ArtistProfileAvailability,
  ArtistReviewContext,
  BeginArtistReview,
  SearchArtistCandidates,
  SelectArtistCandidate,
} from "../../../shared/types";

const part = encodeURIComponent;

export const artistReviewApi = {
  async context(
    account: string,
    recording: string,
    body: BeginArtistReview,
    signal: AbortSignal,
  ): Promise<ArtistReviewContext> {
    return requestJson<ArtistReviewContext>(
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
    body: SearchArtistCandidates,
    signal: AbortSignal,
  ): Promise<ArtistCandidateSearchAvailability> {
    return requestJson<ArtistCandidateSearchAvailability>(
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
    body: SelectArtistCandidate,
    signal: AbortSignal,
  ): Promise<ArtistCandidatePreviewAvailability> {
    return requestJson<ArtistCandidatePreviewAvailability>(
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
  async confirm(
    account: string,
    recording: string,
    body: SelectArtistCandidate,
    signal: AbortSignal,
  ): Promise<ArtistAssociation> {
    return requestJson<ArtistAssociation>(
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
  ): Promise<ArtistAssociation[]> {
    return requestJson<ArtistAssociation[]>(
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
  ): Promise<ArtistProfileAvailability> {
    return requestJson<ArtistProfileAvailability>(
      `/recordings/${part(recording)}/artist-associations/${part(association)}/profile`,
      {
        action: "artist_review.profile",
        account,
        signal,
      },
    );
  },
};
