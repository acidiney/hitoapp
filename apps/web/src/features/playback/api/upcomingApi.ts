import type { Schema } from "../../../shared/schema";
import { request } from "../../../shared/http";

export const upcomingApi = {
  async announce(
    account: string,
    body: Schema<"UpcomingRecordings">,
    signal: AbortSignal,
  ): Promise<void> {
    await request("/playback/upcoming", {
      action: "playback.upcoming",
      method: "POST",
      account,
      body,
      signal,
      errorBody: false,
      timeoutMs: 10_000,
    });
  },
};
