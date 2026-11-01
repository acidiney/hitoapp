import { createElement } from "react";
import type { Schema } from "../../../shared/schema";
import CatalogAlbum from "../components/CatalogAlbum";
import type { ArtistEditionContext } from "../hooks/useArtistEditions";
import { savedRecording } from "../../catalog/fixtures/albumEditions";
import { settings } from "../../metadata/fixtures/metadataPreferences";
import {
  bonusTrackList,
  editionGroup,
  firstEditionPage,
  standardEdition,
} from "./artistEditions";
import { artistAssociation } from "./artistReview";

export const catalogAlbumTitles = [
  "Primeira faixa",
  "Segunda faixa",
  "Terceira faixa",
  "Quarta faixa",
];

const occurrence = (title: string, index: number) => ({
  id: `opaque:occurrence/play/${index + 1}`,
  position: index + 1,
  number: String(index + 1),
  kind: "audio" as const,
  title,
  artist_credit: standardEdition.artist_credit,
  duration_ms: 61_000 + index * 1000,
  recording_id: null,
});

export const catalogAlbumEdition = {
  id: standardEdition.id,
  release_group_id: editionGroup.id,
  title: standardEdition.title,
  artist_credit: standardEdition.artist_credit,
  revision: null,
  media: [
    {
      position: 1,
      title: null,
      format: "CD",
      tracks: catalogAlbumTitles.map(occurrence),
    },
  ],
};

export const catalogAlbumTrackList = {
  status: "available",
  association_id: artistAssociation.id,
  edition: catalogAlbumEdition,
} satisfies Schema<"EditionTrackListAvailability">;

export const oneEditionPage = {
  ...firstEditionPage,
  continuation: null,
  page: { ...firstEditionPage.page, next_cursor: null },
} satisfies Schema<"ReleaseGroupEditionsAvailability">;

export const secondEdition = {
  ...standardEdition,
  id: "opaque:edition/second",
  disambiguation: "Deluxe edition",
};

export const twoEditionsPage = {
  ...oneEditionPage,
  page: {
    ...oneEditionPage.page,
    items: [standardEdition, secondEdition],
  },
} satisfies Schema<"ReleaseGroupEditionsAvailability">;

export const foundRecording = (index: number): Schema<"Recording"> => ({
  ...savedRecording,
  id: `recording-found-${index + 1}`,
  title: catalogAlbumTitles[index],
});

export const automationSettings = (enabled: boolean) => ({
  ...settings([], 1),
  automatic_service_search: enabled,
});

export const catalogAlbumElement = (context: ArtistEditionContext) =>
  createElement(CatalogAlbum, { context, heading: "h4", playOnReady: false });

export const tracksOfEdition = (edition: { id: string }, firstTitle?: string) =>
  ({
    ...bonusTrackList,
    edition: {
      ...bonusTrackList.edition,
      id: edition.id,
      media: bonusTrackList.edition.media.map((medium, index) => ({
        ...medium,
        tracks: medium.tracks.map((track, position) =>
          index === 1 && position === 0 && firstTitle
            ? { ...track, title: firstTitle }
            : track,
        ),
      })),
    },
  }) satisfies Schema<"EditionTrackListAvailability">;
