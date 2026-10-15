import { request } from "./http";
import type {
  InstallPlugin,
  OfferPluginService,
  PluginInstallation,
  PluginRefresh,
  PluginService,
  PluginSettings,
  RefreshPlugin,
} from "./types";
const part = encodeURIComponent;
export const pluginsApi = {
  async settings(
    account: string,
    signal: AbortSignal,
  ): Promise<PluginSettings> {
    return (
      await request("/plugins", { action: "plugins.list", account, signal })
    ).json();
  },
  async install(
    account: string,
    body: InstallPlugin,
    signal: AbortSignal,
  ): Promise<PluginInstallation> {
    return (
      await request("/plugins", {
        action: "plugins.install",
        method: "POST",
        account,
        body,
        signal,
      })
    ).json();
  },
  async offer(
    account: string,
    body: OfferPluginService,
    signal: AbortSignal,
  ): Promise<PluginService> {
    return (
      await request("/plugins/services", {
        action: "plugins.offer",
        method: "POST",
        account,
        body,
        signal,
      })
    ).json();
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
    return (
      await request(`/plugins/${part(plugin.id)}/refresh`, {
        action: "plugins.refresh",
        method: "POST",
        account,
        body,
        signal,
      })
    ).json();
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
    return (
      await request(`/plugins/${part(plugin.id)}/credential`, {
        action: "plugins.clear_credential",
        method: "DELETE",
        account,
        body: { revision: plugin.revision },
        signal,
      })
    ).json();
  },
};
