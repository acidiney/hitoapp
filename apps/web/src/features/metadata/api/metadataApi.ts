import type { Schema } from "../../../shared/schema";
import { requestJson } from "../../../shared/http";

import type { MetadataTarget } from "../model/metadata";

export const metadataApi = {
  async history(
    account: string,
    target: MetadataTarget,
    before: number,
    signal: AbortSignal,
  ): Promise<Schema<"RecordingMetadata"> | Schema<"ReleaseMetadata">> {
    const resource = target.kind === "recording" ? "recordings" : "releases";
    return requestJson<Schema<"RecordingMetadata"> | Schema<"ReleaseMetadata">>(
      `/${resource}/${encodeURIComponent(target.id)}/metadata?before=${before}`,
      {
        action: "metadata.history",
        account,
        signal,
      },
    );
  },
};
