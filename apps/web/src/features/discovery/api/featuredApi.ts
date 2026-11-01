import type { Schema } from "../../../shared/schema";
import { requestJson } from "../../../shared/http";

export const featuredApi = {
  read(account: string, signal: AbortSignal): Promise<Schema<"FeaturedShelf">> {
    return requestJson<Schema<"FeaturedShelf">>("/discover/featured", {
      action: "discover.featured",
      account,
      signal,
    });
  },
  artworkUrl(entry: Schema<"FeaturedEntry">): string | null {
    if (!entry.artwork_id) return null;
    return `/api/v1/plugins/${encodeURIComponent(entry.installation_id)}/featured-artwork?revision=${entry.installation_revision}&artwork=${encodeURIComponent(entry.artwork_id)}`;
  },
};
