import { requestJson } from "../../../shared/http";
import type {
  MetadataSettings,
  UpdateMetadataSettings,
} from "../../../shared/types";

export const metadataPreferencesApi = {
  async settings(
    account: string,
    signal: AbortSignal,
  ): Promise<MetadataSettings> {
    return requestJson<MetadataSettings>("/metadata/settings", {
      action: "metadata.settings",
      account,
      signal,
    });
  },
  async update(
    account: string,
    body: UpdateMetadataSettings,
    signal: AbortSignal,
  ): Promise<MetadataSettings> {
    return requestJson<MetadataSettings>("/metadata/settings", {
      action: "metadata.settings.update",
      method: "PUT",
      account,
      body,
      signal,
    });
  },
};
