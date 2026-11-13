import type { Schema } from "../../../shared/schema";
import {
  editionGroup,
  firstEditionPage,
  standardEdition,
} from "../../artists/fixtures/artistEditions";
import { firstDiscographyPage } from "../../artists/fixtures/artistDiscography";
import {
  artistAssociation,
  artistReviewAccount,
  artistReviewDetail,
  reviewedArtistProfile,
} from "../../artists/fixtures/artistReview";

export {
  artistAssociation,
  artistReviewAccount,
  editionGroup,
  firstDiscographyPage,
  firstEditionPage,
  reviewedArtistProfile,
  standardEdition,
};

export const albumId = "album-with-catalog";
export const personalTitle = "Private personal opening";
export const installation = { id: "installation-1", revision: 7 };

const recording = (id: string, title: string, personal = title) => ({
  id,
  library_id: "personal",
  release_id: albumId,
  title: personal,
  inherited_title: title,
  artist: "Performer",
  inherited_artist: "Performer",
  album: "Catalog album",
  duration_ms: 1000,
  available: false,
  plays_from_services: true,
  liked: false,
});

export const firstLibraryRecording = recording(
  artistReviewDetail.recording_id,
  "Opening",
  personalTitle,
);
export const extraLibraryRecording = recording(
  "recording-extra",
  "Library only",
);
export const savedRecording = recording(
  "recording-saved-from-catalog",
  "Catalog only song",
);

const extraTrack = {
  ...extraLibraryRecording,
  entry_id: "entry-extra",
  disc_number: 1,
  track_number: 9,
};

export const albumDetail = {
  release: {
    id: albumId,
    library_id: "personal",
    title: "Catalog album",
    artist: "Performer",
    artist_id: "artist",
    inferred_artist: null,
    year: null,
    track_count: 2,
    duration_ms: 2000,
    artwork_recording_id: firstLibraryRecording.id,
  },
  tracks: {
    total: 2,
    offset: 0,
    limit: 60,
    items: [
      {
        ...firstLibraryRecording,
        entry_id: "entry-one",
        disc_number: 1,
        track_number: 1,
      },
      extraTrack,
    ],
  },
  catalog_anchor: {
    recording_id: firstLibraryRecording.id,
    association_id: artistAssociation.id,
  },
  saved_edition: { revision: 0, state: { status: "unset" } },
} satisfies Schema<"ReleaseDetail">;

export const savedEdition = {
  recording_id: firstLibraryRecording.id,
  association_id: artistAssociation.id,
  release_group_id: editionGroup.id,
  edition_id: standardEdition.id,
} satisfies Schema<"SavedAlbumEdition">;

export const readyChoice = {
  revision: 3,
  state: { status: "ready", edition: savedEdition },
} satisfies Schema<"AlbumEditionChoice">;

export const staleChoice = {
  revision: 3,
  state: { status: "needs_choice" },
} satisfies Schema<"AlbumEditionChoice">;

export const choiceReceipt = (revision: number, edition = savedEdition) =>
  ({
    request_id: "00000000-0000-4000-8000-000000000001",
    release_id: albumId,
    revision,
    edition,
  }) satisfies Schema<"AlbumEditionChoiceReceipt">;

const occurrence = (
  id: string,
  position: number,
  kind: "audio" | "pregap" | "data",
  title: string,
  recording_id: string | null,
) => ({
  id,
  position,
  number: position === 0 ? null : String(position),
  kind,
  title,
  artist_credit: null,
  duration_ms: 1000,
  recording_id,
});

export const catalogTracks = {
  id: standardEdition.id,
  release_group_id: editionGroup.id,
  title: standardEdition.title,
  artist_credit: null,
  revision: null,
  media: [
    {
      position: 1,
      title: "Main disc",
      format: "CD",
      tracks: [
        occurrence("occ-1-0", 0, "pregap", "Hidden intro", null),
        occurrence("occ-1-1", 1, "audio", "Catalog opening", "provider:one"),
        occurrence("occ-1-2", 2, "audio", "Catalog only song", "provider:two"),
      ],
    },
    {
      position: 2,
      title: "Bonus disc",
      format: null,
      tracks: [
        occurrence("occ-2-1", 1, "audio", "Catalog opening", "provider:one"),
        occurrence("occ-2-2", 2, "audio", "Unidentified bonus", null),
      ],
    },
  ],
} satisfies Schema<"CatalogEditionTrackList">;

export const mergedTrackList = {
  status: "available",
  association_id: artistAssociation.id,
  edition: catalogTracks,
  matches: [
    { occurrence_id: "occ-1-1", recording: firstLibraryRecording },
    { occurrence_id: "occ-2-1", recording: firstLibraryRecording },
  ],
  library_only: [extraTrack],
  attached_occurrences: [],
  adoption: {
    installation_id: installation.id,
    installation_revision: installation.revision,
  },
  is_album_release: true,
  stats: { audio_tracks: 4, total_duration_ms: 4000 },
} satisfies Schema<"AlbumEditionTrackListAvailability">;

export const unsavableTrackList = {
  ...mergedTrackList,
  matches: [],
  adoption: null,
  is_album_release: false,
} satisfies Schema<"AlbumEditionTrackListAvailability">;

export const attachedTrackList = {
  ...mergedTrackList,
  attached_occurrences: ["occ-1-2"],
} satisfies Schema<"AlbumEditionTrackListAvailability">;

export const repeatedCatalogTrackList = {
  ...mergedTrackList,
  matches: [],
  edition: {
    ...catalogTracks,
    media: [
      catalogTracks.media[0],
      {
        ...catalogTracks.media[1],
        tracks: [
          occurrence(
            "occ-2-1",
            1,
            "audio",
            "Catalog song again",
            "provider:two",
          ),
          catalogTracks.media[1].tracks[1],
        ],
      },
    ],
  },
} satisfies Schema<"AlbumEditionTrackListAvailability">;
