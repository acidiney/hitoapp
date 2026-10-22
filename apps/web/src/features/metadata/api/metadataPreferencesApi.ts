import { request } from "../../../shared/http";
import type { MetadataSettings, UpdateMetadataSettings } from "../../../shared/types";

export const metadataPreferencesApi = {
  async settings(
    account: string,
    signal: AbortSignal,
  ): Promise<MetadataSettings> {
    return (
      await request("/metadata/settings", {
        action: "metadata.settings",
        account,
        signal,
      })
    ).json();
  },
  async update(
    account: string,
    body: UpdateMetadataSettings,
    signal: AbortSignal,
  ): Promise<MetadataSettings> {
    return (
      await request("/metadata/settings", {
        action: "metadata.settings.update",
        method: "PUT",
        account,
        body,
        signal,
      })
    ).json();
  },
};
