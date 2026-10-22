import { request, requestJson } from "../../../shared/http";
import type {
  InstallPlugin,
  OfferPluginService,
  PluginInstallation,
  PluginInstallationPreview,
  PreviewPluginInstallation,
  PluginRefresh,
  PluginService,
  PluginSettings,
  RefreshPlugin,
  ProviderSearch,
  ProviderSearchPage,
  AdoptProviderRecording,
  Recording,
  ImportProviderPlaylist,
  PlaylistImportReceipt,
  FollowProviderPlaylist,
  PlaylistFollowReceipt,
} from "../../../shared/types";
const part = encodeURIComponent;
export const pluginsApi = {
  async followPlaylist(
    account: string,
    installation: string,
    body: FollowProviderPlaylist,
    signal: AbortSignal,
  ): Promise<PlaylistFollowReceipt> {
    return requestJson<PlaylistFollowReceipt>(
      `/plugins/${part(installation)}/playlists/follow`,
      {
        action: "playlists.follow",
        method: "POST",
        account,
        body,
        signal,
      },
    );
  },
  async importPlaylist(
    account: string,
    installation: string,
    body: ImportProviderPlaylist,
    signal: AbortSignal,
  ): Promise<PlaylistImportReceipt> {
    return requestJson<PlaylistImportReceipt>(
      `/plugins/${part(installation)}/playlists/import`,
      {
        action: "playlists.import",
        method: "POST",
        account,
        body,
        signal,
      },
    );
  },
  async adopt(
    account: string,
    installation: string,
    body: AdoptProviderRecording,
    signal: AbortSignal,
  ): Promise<Recording> {
    return requestJson<Recording>(`/plugins/${part(installation)}/recordings`, {
      action: "providers.adopt",
      method: "POST",
      account,
      body,
      signal,
    });
  },
  async search(
    account: string,
    installation: string,
    body: ProviderSearch,
    signal: AbortSignal,
  ): Promise<ProviderSearchPage> {
    return requestJson<ProviderSearchPage>(
      `/plugins/${part(installation)}/search`,
      {
        action: "providers.search",
        method: "POST",
        account,
        body,
        signal,
      },
    );
  },
  async settings(
    account: string,
    signal: AbortSignal,
  ): Promise<PluginSettings> {
    return requestJson<PluginSettings>("/plugins", {
      action: "plugins.list",
      account,
      signal,
    });
  },
  async install(
    account: string,
    body: InstallPlugin,
    signal: AbortSignal,
  ): Promise<PluginInstallation> {
    return requestJson<PluginInstallation>("/plugins", {
      action: "plugins.install",
      method: "POST",
      account,
      body,
      signal,
    });
  },
  async preview(
    account: string,
    body: PreviewPluginInstallation,
    signal: AbortSignal,
  ): Promise<PluginInstallationPreview> {
    return requestJson<PluginInstallationPreview>("/plugins/preview", {
      action: "plugins.preview",
      method: "POST",
      account,
      body,
      signal,
    });
  },
  async offer(
    account: string,
    body: OfferPluginService,
    signal: AbortSignal,
  ): Promise<PluginService> {
    return requestJson<PluginService>("/plugins/services", {
      action: "plugins.offer",
      method: "POST",
      account,
      body,
      signal,
    });
  },
  async withdraw(
    account: string,
    service: PluginService,
    signal: AbortSignal,
  ): Promise<void> {
    await request(`/plugins/services/${part(service.id)}`, {
      action: "plugins.withdraw",
      method: "DELETE",
      account,
      body: { revision: service.revision },
      signal,
    });
  },
  async refresh(
    account: string,
    plugin: PluginInstallation,
    body: RefreshPlugin,
    signal: AbortSignal,
  ): Promise<PluginRefresh> {
    return requestJson<PluginRefresh>(`/plugins/${part(plugin.id)}/refresh`, {
      action: "plugins.refresh",
      method: "POST",
      account,
      body,
      signal,
    });
  },
  async remove(
    account: string,
    plugin: PluginInstallation,
    signal: AbortSignal,
  ): Promise<void> {
    await request(`/plugins/${part(plugin.id)}`, {
      action: "plugins.remove",
      method: "DELETE",
      account,
      body: { revision: plugin.revision },
      signal,
    });
  },
  async clearCredential(
    account: string,
    plugin: PluginInstallation,
    signal: AbortSignal,
  ): Promise<PluginInstallation> {
    return requestJson<PluginInstallation>(
      `/plugins/${part(plugin.id)}/credential`,
      {
        action: "plugins.clear_credential",
        method: "DELETE",
        account,
        body: { revision: plugin.revision },
        signal,
      },
    );
  },
};
