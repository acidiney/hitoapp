import type { Schema } from "../../../shared/schema";
import { requestJson } from "../../../shared/http";

export const metadataPreferencesApi = {
  async settings(
    account: string,
    signal: AbortSignal,
  ): Promise<Schema<"MetadataSettings">> {
    return requestJson<Schema<"MetadataSettings">>("/metadata/settings", {
      action: "metadata.settings",
      account,
      signal,
    });
  },
  async update(
    account: string,
    body: Schema<"UpdateMetadataSettings">,
    signal: AbortSignal,
  ): Promise<Schema<"MetadataSettings">> {
    return requestJson<Schema<"MetadataSettings">>("/metadata/settings", {
      action: "metadata.settings.update",
      method: "PUT",
      account,
      body,
      signal,
    });
  },
};
