import { request } from "../../../shared/http";
import type {
  ArtistDiscographyAvailability,
  ReadArtistDiscography,
} from "../../../shared/types";

const part = encodeURIComponent;

export const artistDiscographyApi = {
  async read(
    account: string,
    recording: string,
    association: string,
    body: ReadArtistDiscography,
    signal: AbortSignal,
  ): Promise<ArtistDiscographyAvailability> {
    return (
      await request(
        `/recordings/${part(recording)}/artist-associations/${part(association)}/discography`,
        {
          action: "artist_discography.read",
          method: "POST",
          account,
          body,
          signal,
        },
      )
    ).json();
  },
};
