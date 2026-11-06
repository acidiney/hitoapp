import type { Schema } from "../../../shared/schema";
import { request } from "../../../shared/http";

export const albumListApi = {
  async releases(
    account: string,
    filter: { query: string },
    offset: number,
    signal: AbortSignal,
  ): Promise<Schema<"Page_Release">> {
    const parameters = new URLSearchParams();
    if (filter.query) parameters.set("q", filter.query);
    parameters.set("offset", String(offset));
    const response = await request(`/releases?${parameters}`, {
      action: "albums.list",
      account,
      signal,
    });
    if (signal.aborted) {
      await response.body?.cancel();
      signal.throwIfAborted();
    }
    return response.json();
  },
};
