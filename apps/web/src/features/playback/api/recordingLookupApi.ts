import type { Schema } from "../../../shared/schema";
import { requestJson } from "../../../shared/http";

export const recordingLookupApi = {
  async read(
    account: string,
    recordings: string[],
    signal?: AbortSignal,
  ): Promise<Schema<"Recording">[]> {
    return requestJson<Schema<"Recording">[]>("/recordings/lookup", {
      action: "recordings.lookup",
      method: "POST",
      body: { recording_ids: recordings },
      account,
      signal,
    });
  },
};
