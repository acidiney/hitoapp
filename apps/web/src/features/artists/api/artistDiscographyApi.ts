import type { Schema } from "../../../shared/schema";
import { requestJson } from "../../../shared/http";
import { cachedRead, rememberReleaseGroups } from "./catalogCache";

const part = encodeURIComponent;

export const artistDiscographyApi = {
  async read(
    account: string,
    recording: string,
    association: string,
    body: Schema<"ReadArtistDiscography">,
    signal: AbortSignal,
    fresh = false,
  ): Promise<Schema<"ArtistDiscographyAvailability">> {
    const result = await cachedRead(
      account,
      ["discography", recording, association, body],
      (loadSignal) =>
        requestJson<Schema<"ArtistDiscographyAvailability">>(
          `/recordings/${part(recording)}/artist-associations/${part(association)}/discography`,
          {
            action: "artist_discography.read",
            method: "POST",
            account,
            body,
            signal: loadSignal,
          },
        ),
      signal,
      fresh,
    );
    if (result.status === "available")
      rememberReleaseGroups(account, association, result.page.items);
    return result;
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
