import { request } from "../../../shared/http";
import type { RecordingMetadata, ReleaseMetadata } from "../../../shared/types";
import type { MetadataTarget } from "../metadata";

export const metadataApi = {
  async history(
    account: string,
    target: MetadataTarget,
    before: number,
    signal: AbortSignal,
  ): Promise<RecordingMetadata | ReleaseMetadata> {
    const resource = target.kind === "recording" ? "recordings" : "releases";
    return (
      await request(
        `/${resource}/${encodeURIComponent(target.id)}/metadata?before=${before}`,
        {
          action: "metadata.history",
          account,
          signal,
        },
      )
    ).json();
  },
};
