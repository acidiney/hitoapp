import type { Schema } from "../../../shared/schema";
import { request, requestJson } from "../../../shared/http";
import { invalidate } from "../../../shared/invalidation";
import { clearCredentialRequirement } from "../model/credentialRequirements";

const part = encodeURIComponent;
export const pluginsApi = {
  async connectionStatus(
    account: string,
    installation: string,
    signal: AbortSignal,
  ): Promise<Schema<"PluginConnectionStatus">> {
    return requestJson<Schema<"PluginConnectionStatus">>(
      `/plugins/${part(installation)}/connection`,
      {
        action: "plugins.connection",
        account,
        signal,
        cache: "no-store",
      },
    );
  },
  async beginAuthorization(
    account: string,
    plugin: Schema<"PluginInstallation">,
    signal: AbortSignal,
  ): Promise<Schema<"PluginAuthorization">> {
    return requestJson<Schema<"PluginAuthorization">>(
      `/plugins/${part(plugin.id)}/authorization`,
      {
        action: "plugins.authorize",
        method: "POST",
        account,
        body: { revision: plugin.revision },
        signal,
      },
    );
  },
  async authorizationStatus(
    account: string,
    installation: string,
    attempt: string,
    signal: AbortSignal,
  ): Promise<Schema<"PluginAuthorizationStatus">> {
    const status = await requestJson<Schema<"PluginAuthorizationStatus">>(
      `/plugins/${part(installation)}/authorization/${part(attempt)}`,
      {
        action: "plugins.authorization_status",
        account,
        signal,
        cache: "no-store",
      },
    );
    if (status.status === "completed") invalidate("plugins", installation);
    return status;
  },
  async launchAuthorization(
    account: string,
    installation: string,
    attempt: string,
    authorizationUrl: string,
    signal: AbortSignal,
  ): Promise<void> {
    await request(
      `/plugins/${part(installation)}/authorization/${part(attempt)}/launch`,
      {
        action: "plugins.authorization_launch",
        method: "POST",
        account,
        body: {
          authorization_url: authorizationUrl,
        } satisfies Schema<"AuthorizationLaunch">,
        signal,
      },
    );
  },
  async disconnect(
    account: string,
    installation: string,
    signal: AbortSignal,
  ): Promise<void> {
    await request(`/plugins/${part(installation)}/connection`, {
      action: "plugins.disconnect",
      method: "DELETE",
      account,
      signal,
    });
    invalidate("plugins", installation);
  },
  async followPlaylist(
    account: string,
    installation: string,
    body: Schema<"FollowProviderPlaylist">,
    signal: AbortSignal,
  ): Promise<Schema<"PlaylistFollowReceipt">> {
    return requestJson<Schema<"PlaylistFollowReceipt">>(
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
    body: Schema<"ImportProviderPlaylist">,
    signal: AbortSignal,
  ): Promise<Schema<"PlaylistImportReceipt">> {
    return requestJson<Schema<"PlaylistImportReceipt">>(
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
  async openPlaylist(
    account: string,
    installation: string,
    body: Schema<"ImportProviderPlaylist">,
    signal: AbortSignal,
  ): Promise<Schema<"OpenedProviderPlaylist">> {
    return requestJson<Schema<"OpenedProviderPlaylist">>(
      `/plugins/${part(installation)}/playlists/open`,
      {
        action: "playlists.open",
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
    body: Schema<"AdoptProviderRecording">,
    signal: AbortSignal,
  ): Promise<Schema<"Recording">> {
    return requestJson<Schema<"Recording">>(
      `/plugins/${part(installation)}/recordings`,
      {
        action: "providers.adopt",
        method: "POST",
        account,
        body,
        signal,
      },
    );
  },
  async search(
    account: string,
    installation: string,
    body: Schema<"ProviderSearch">,
    signal: AbortSignal,
  ): Promise<Schema<"SearchPage">> {
    return requestJson<Schema<"SearchPage">>(
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
  ): Promise<Schema<"PluginSettings">> {
    return requestJson<Schema<"PluginSettings">>("/plugins", {
      action: "plugins.list",
      account,
      signal,
    });
  },
  async install(
    account: string,
    body: Schema<"InstallPlugin">,
    signal: AbortSignal,
  ): Promise<Schema<"PluginInstallation">> {
    return requestJson<Schema<"PluginInstallation">>("/plugins", {
      action: "plugins.install",
      method: "POST",
      account,
      body,
      signal,
    });
  },
  async preview(
    account: string,
    body: Schema<"PreviewPluginInstallation">,
    signal: AbortSignal,
  ): Promise<Schema<"PluginInstallationPreview">> {
    return requestJson<Schema<"PluginInstallationPreview">>(
      "/plugins/preview",
      {
        action: "plugins.preview",
        method: "POST",
        account,
        body,
        signal,
      },
    );
  },
  async offer(
    account: string,
    body: Schema<"OfferPluginService">,
    signal: AbortSignal,
  ): Promise<Schema<"PluginService">> {
    return requestJson<Schema<"PluginService">>("/plugins/services", {
      action: "plugins.offer",
      method: "POST",
      account,
      body,
      signal,
    });
  },
  async withdraw(
    account: string,
    service: Schema<"PluginService">,
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
    plugin: Schema<"PluginInstallation">,
    body: Schema<"RefreshPlugin">,
    signal: AbortSignal,
  ): Promise<Schema<"PluginRefresh">> {
    const result = await requestJson<Schema<"PluginRefresh">>(
      `/plugins/${part(plugin.id)}/refresh`,
      {
        action: "plugins.refresh",
        method: "POST",
        account,
        body,
        signal,
      },
    );
    if (result.kind === "updated" && body.credential?.action === "replace") {
      clearCredentialRequirement(account, plugin.id);
      invalidate("plugins", plugin.id);
      invalidate("lyrics");
    }
    return result;
  },
  async checkDeclaration(
    account: string,
    installation: string,
    revision: number,
    signal: AbortSignal,
  ): Promise<Schema<"PluginDeclarationCheck">> {
    return requestJson<Schema<"PluginDeclarationCheck">>(
      `/plugins/${part(installation)}/declaration-check`,
      {
        action: "plugins.declaration_check",
        method: "POST",
        account,
        body: { revision },
        signal,
      },
    );
  },
  async remove(
    account: string,
    plugin: Schema<"PluginInstallation">,
    signal: AbortSignal,
  ): Promise<void> {
    await request(`/plugins/${part(plugin.id)}`, {
      action: "plugins.remove",
      method: "DELETE",
      account,
      body: { revision: plugin.revision },
      signal,
    });
    invalidate("plugins", plugin.id);
  },
  async clearCredential(
    account: string,
    plugin: Schema<"PluginInstallation">,
    signal: AbortSignal,
  ): Promise<Schema<"PluginInstallation">> {
    return requestJson<Schema<"PluginInstallation">>(
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
