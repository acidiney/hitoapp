import type { components } from "../generated";
import { artistAssociation, reviewedArtistProfile } from "./artistReview";

type Schema = components["schemas"];

export const hostDiscographyContinuation =
  "private-hito-discography-continuation:2";
export const providerDiscographyCursor =
  "private-provider-discography-cursor:2";
export const discographyTitle = "Sessão nº 1";
export const discographyCredit = "O conjunto da sessão & Voz convidada";

export const firstDiscographyPage = {
  status: "available",
  association_id: artistAssociation.id,
  continuation: hostDiscographyContinuation,
  page: {
    artist_id: reviewedArtistProfile.profile.artist.id,
    snapshot: null,
    total_count: null,
    next_cursor: providerDiscographyCursor,
    items: [
      {
        id: "opaque:release-group/ao-vivo?edição=一",
        title: discographyTitle,
        artist_credit: [
          {
            artist: reviewedArtistProfile.profile.artist,
            credited_name: "O conjunto da sessão",
            join_phrase: " & ",
          },
          {
            artist: null,
            credited_name: "Voz convidada",
            join_phrase: "",
          },
        ],
        kind: "album",
        secondary_types: ["Live"],
        first_release_date: "2024-02",
        revision: null,
      },
    ],
  },
} satisfies Schema["ArtistDiscographyAvailability"];

export const lastDiscographyPage = {
  status: "available",
  association_id: artistAssociation.id,
  continuation: null,
  page: {
    artist_id: reviewedArtistProfile.profile.artist.id,
    snapshot: null,
    total_count: null,
    next_cursor: null,
    items: [
      {
        id: "opaque:release-group/estúdio?edição=二",
        title: discographyTitle,
        artist_credit: null,
        kind: null,
        secondary_types: [],
        first_release_date: null,
        revision: null,
      },
    ],
  },
} satisfies Schema["ArtistDiscographyAvailability"];
