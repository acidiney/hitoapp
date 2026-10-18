import { request } from "./http";
import type {
  ArtistAssociation,
  ArtistCandidatePreviewAvailability,
  ArtistCandidateSearchAvailability,
  ArtistProfileAvailability,
  ArtistReviewContext,
  BeginArtistReview,
  SearchArtistCandidates,
  SelectArtistCandidate,
} from "./types";

const part = encodeURIComponent;

export const artistReviewApi = {
  async context(
    account: string,
    recording: string,
    body: BeginArtistReview,
    signal: AbortSignal,
  ): Promise<ArtistReviewContext> {
    return (
      await request(`/recordings/${part(recording)}/artist-review/context`, {
        action: "artist_review.context",
        method: "POST",
        account,
        body,
        signal,
      })
    ).json();
  },
  async search(
    account: string,
    recording: string,
    body: SearchArtistCandidates,
    signal: AbortSignal,
  ): Promise<ArtistCandidateSearchAvailability> {
    return (
      await request(`/recordings/${part(recording)}/artist-review/search`, {
        action: "artist_review.search",
        method: "POST",
        account,
        body,
        signal,
      })
    ).json();
  },
  async preview(
    account: string,
    recording: string,
    body: SelectArtistCandidate,
    signal: AbortSignal,
  ): Promise<ArtistCandidatePreviewAvailability> {
    return (
      await request(`/recordings/${part(recording)}/artist-review/preview`, {
        action: "artist_review.preview",
        method: "POST",
        account,
        body,
        signal,
      })
    ).json();
  },
  async confirm(
    account: string,
    recording: string,
    body: SelectArtistCandidate,
    signal: AbortSignal,
  ): Promise<ArtistAssociation> {
    return (
      await request(`/recordings/${part(recording)}/artist-review/confirm`, {
        action: "artist_review.confirm",
        method: "POST",
        account,
        body,
        signal,
      })
    ).json();
  },
  async associations(
    account: string,
    recording: string,
    signal: AbortSignal,
  ): Promise<ArtistAssociation[]> {
    return (
      await request(`/recordings/${part(recording)}/artist-associations`, {
        action: "artist_review.associations",
        account,
        signal,
      })
    ).json();
  },
  async profile(
    account: string,
    recording: string,
    association: string,
    signal: AbortSignal,
  ): Promise<ArtistProfileAvailability> {
    return (
      await request(
        `/recordings/${part(recording)}/artist-associations/${part(association)}/profile`,
        {
          action: "artist_review.profile",
          account,
          signal,
        },
      )
    ).json();
  },
};
