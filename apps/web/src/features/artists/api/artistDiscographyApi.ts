import type { Schema } from "../../../shared/schema";
import { requestJson } from "../../../shared/http";

const part = encodeURIComponent;

export const artistDiscographyApi = {
  async read(
    account: string,
    recording: string,
    association: string,
    body: Schema<"ReadArtistDiscography">,
    signal: AbortSignal,
  ): Promise<Schema<"ArtistDiscographyAvailability">> {
    return requestJson<Schema<"ArtistDiscographyAvailability">>(
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
    body: Schema<"ReadDiscographyLibrary">,
    signal: AbortSignal,
  ): Promise<Schema<"DiscographyLibrary">> {
    return requestJson<Schema<"DiscographyLibrary">>(
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
