import { requestJson } from "../../../shared/http";
import type {
  ArtistDiscographyAvailability,
  DiscographyLibrary,
  ReadDiscographyLibrary,
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
    return requestJson<ArtistDiscographyAvailability>(
      `/recordings/${part(recording)}/artist-associations/${part(association)}/discography`,
      {
        action: "artist_discography.read",
        method: "POST",
        account,
        body,
        signal,
      },
    );
  },
  async library(
    account: string,
    recording: string,
    association: string,
    body: ReadDiscographyLibrary,
    signal: AbortSignal,
  ): Promise<DiscographyLibrary> {
    return requestJson<DiscographyLibrary>(
      `/recordings/${part(recording)}/artist-associations/${part(association)}/discography/library`,
      {
        action: "artist_discography.library",
        method: "POST",
        account,
        body,
        signal,
      },
    );
  },
};
