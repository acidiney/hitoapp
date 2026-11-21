import type { Schema } from "../../../shared/schema";
import { requestJson } from "../../../shared/http";

export const sameRecordingApi = {
  suggest(
    account: string,
    source: string,
    request: Schema<"SameRecording">,
    signal: AbortSignal,
  ): Promise<Schema<"CommunityReportAdmission">> {
    return requestJson<Schema<"CommunityReportAdmission">>(
      `/sources/${encodeURIComponent(source)}/same-recording`,
      {
        action: "community.same_recording",
        method: "POST",
        account,
        body: request,
        signal,
      },
    );
  },
};
