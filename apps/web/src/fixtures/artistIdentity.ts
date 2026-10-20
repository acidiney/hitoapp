import type { components } from "../generated";
import { artistAssociation, artistReviewDetail } from "./artistReview";
import { savedRecording } from "./sourceAttachment";

type Schema = components["schemas"];
export const artistIdentityGroup = "70000000-0000-4000-8000-000000000001";
export const unselectedArtistIdentity = {
  recording_id: artistReviewDetail.recording_id,
  revision: 0,
  state: { status: "unselected" },
} satisfies Schema["RecordingArtistIdentityChoice"];
export const selectedArtistIdentity = {
  recording_id: artistReviewDetail.recording_id,
  revision: 1,
  state: {
    status: "selected",
    group_id: artistIdentityGroup,
    association_id: artistAssociation.id,
  },
} satisfies Schema["RecordingArtistIdentityChoice"];
export const artistIdentityGroupDetail = {
  group: {
    id: artistIdentityGroup,
    provider: artistAssociation.provider,
    provider_artist_id: artistAssociation.provider_artist_id,
    name: {
      value: "Saved credit shared across selected tracks",
      recording_id: artistReviewDetail.recording_id,
      artist_id: artistAssociation.artist_id,
      association_id: artistAssociation.id,
    },
    library_count: 1,
    recording_count: 1,
    release_count: 0,
  },
  libraries: {
    items: [
      {
        library_id: artistAssociation.library_id,
        recording_count: 1,
        release_count: 0,
      },
    ],
    offset: 0,
    limit: 1,
    total: 1,
  },
  recordings: {
    items: [
      {
        recording: savedRecording,
        artist_id: artistAssociation.artist_id,
        association_id: artistAssociation.id,
        choice_revision: 1,
      },
    ],
    offset: 0,
    limit: 1,
    total: 1,
  },
  releases: { items: [], offset: 0, limit: 1, total: 0 },
} satisfies Schema["ArtistIdentityGroupDetail"];
