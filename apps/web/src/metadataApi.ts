import { request } from "./http";
import type { RecordingMetadata } from "./types";

export const metadataApi = {
  async recording(
    account: string,
    recording: string,
    before: number,
    signal: AbortSignal,
  ): Promise<RecordingMetadata> {
    return (
      await request(
        `/recordings/${encodeURIComponent(recording)}/metadata?before=${before}`,
        {
          action: "metadata.history",
          account,
          signal,
        },
      )
    ).json();
  },
};
