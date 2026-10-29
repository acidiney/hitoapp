import { requestJson } from "../../../shared/http";
import type {
  ReadSimilarArtists,
  SimilarArtistsAvailability,
} from "../../../shared/types";

const part = encodeURIComponent;

export const similarArtistsApi = {
  async read(
    account: string,
    recording: string,
    association: string,
    body: ReadSimilarArtists,
    signal: AbortSignal,
  ): Promise<SimilarArtistsAvailability> {
    return requestJson<SimilarArtistsAvailability>(
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
