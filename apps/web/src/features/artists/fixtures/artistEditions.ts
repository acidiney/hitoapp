import type { components } from "../../../generated";
import { firstDiscographyPage } from "./artistDiscography";
import { artistAssociation, reviewedArtistProfile } from "./artistReview";

type Schema = components["schemas"];

export const editionGroup = firstDiscographyPage.page.items[0];
export const editionTitle = "Sessão nº 1";
export const hostEditionContinuation = "private-hito-edition-continuation:2";
export const providerEditionCursor = "private-provider-edition-cursor:2";
export const standardEdition = {
  id: "opaque:edition/padrão?versão=一",
  title: editionTitle,
  artist_credit: [
    {
      artist: reviewedArtistProfile.profile.artist,
      credited_name: "O conjunto da sessão",
      join_phrase: "",
    },
  ],
  disambiguation: "Standard edition",
  release_date: "2024-02",
  country: "PT",
  revision: null,
} satisfies Schema["CatalogEditionSummary"];
export const bonusEdition = {
  id: "opaque:edition/bónus?versão=二",
  title: editionTitle,
  artist_credit: null,
  disambiguation: "Bonus edition",
  release_date: null,
  country: null,
  revision: "private-edition-revision:bonus",
} satisfies Schema["CatalogEditionSummary"];

export const firstEditionPage = {
  status: "available",
  association_id: artistAssociation.id,
  continuation: hostEditionContinuation,
  page: {
    release_group_id: editionGroup.id,
    snapshot: "private-edition-snapshot:one",
    items: [standardEdition],
    total_count: null,
    next_cursor: providerEditionCursor,
  },
} satisfies Schema["ReleaseGroupEditionsAvailability"];

export const lastEditionPage = {
  ...firstEditionPage,
  continuation: null,
  page: {
    ...firstEditionPage.page,
    items: [bonusEdition],
    next_cursor: null,
  },
} satisfies Schema["ReleaseGroupEditionsAvailability"];

export const bonusTrackList = {
  status: "available",
  association_id: artistAssociation.id,
  edition: {
    id: bonusEdition.id,
    release_group_id: editionGroup.id,
    title: editionTitle,
    artist_credit: null,
    revision: bonusEdition.revision,
    media: [
      {
        position: 1,
        title: "The session",
        format: "CD",
        tracks: [
          {
            id: "opaque:occurrence/bonus/1/0",
            position: 0,
            number: "0",
            kind: "pregap",
            title: "Introdução de ensaio",
            artist_credit: null,
            duration_ms: null,
            recording_id: null,
          },
          {
            id: "opaque:occurrence/bonus/1/1",
            position: 1,
            number: "1",
            kind: "audio",
            title: "Tema repetido",
            artist_credit: standardEdition.artist_credit,
            duration_ms: 125_000,
            recording_id: "opaque:recording/repeated?take=一",
          },
        ],
      },
      {
        position: 2,
        title: null,
        format: null,
        tracks: [
          {
            id: "opaque:occurrence/bonus/2/1",
            position: 1,
            number: "A1",
            kind: "audio",
            title: "Tema repetido",
            artist_credit: null,
            duration_ms: 126_000,
            recording_id: "opaque:recording/repeated?take=一",
          },
          {
            id: "opaque:occurrence/bonus/2/2",
            position: 2,
            number: null,
            kind: "data",
            title: "Material de sessão",
            artist_credit: null,
            duration_ms: null,
            recording_id: null,
          },
        ],
      },
    ],
  },
} satisfies Schema["EditionTrackListAvailability"];
