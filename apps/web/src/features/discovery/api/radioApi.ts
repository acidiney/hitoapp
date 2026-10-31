import type { Schema } from "../../../shared/schema";
import { requestJson } from "../../../shared/http";

export const radioApi = {
  start(
    account: string,
    group: string,
    signal: AbortSignal,
  ): Promise<Schema<"ArtistRadio">> {
    return requestJson<Schema<"ArtistRadio">>(
      `/artist-identity-groups/${encodeURIComponent(group)}/radio`,
      { action: "artist_radio.start", method: "POST", account, signal },
    );
  },
};
