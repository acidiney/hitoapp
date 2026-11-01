import type { Schema } from "../../../shared/schema";
import { requestJson } from "../../../shared/http";

export const recordingCopiesApi = {
  async split(
    account: string,
    recording: string,
    signal?: AbortSignal,
  ): Promise<Schema<"RecordingSplit">> {
    return requestJson<Schema<"RecordingSplit">>(
      `/recordings/${encodeURIComponent(recording)}/copies/split`,
      { action: "recording_copies.split", method: "POST", account, signal },
    );
  },
};
