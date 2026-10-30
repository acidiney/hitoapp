import type { Schema } from "../../../shared/schema";
import { requestJson } from "../../../shared/http";

const part = encodeURIComponent;

export const similarArtistsApi = {
  async read(
    account: string,
    recording: string,
    association: string,
    body: Schema<"ReadSimilarArtists">,
    signal: AbortSignal,
  ): Promise<Schema<"SimilarArtistsAvailability">> {
    return requestJson<Schema<"SimilarArtistsAvailability">>(
      `/recordings/${part(recording)}/artist-associations/${part(association)}/similar-artists`,
      {
        action: "similar_artists.read",
        method: "POST",
        account,
        body,
        signal,
      },
    );
  },
};
