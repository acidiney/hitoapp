import { createElement } from "react";
import MetadataPreferences from "../components/metadata/MetadataPreferences";
import type {
  MetadataProvider,
  MetadataSettings,
  PluginInstallation,
  PluginService,
  PluginSettings,
} from "../types";

export const builtin: MetadataProvider = { kind: "builtin_musicbrainz" };

export function settings(
  providers: MetadataProvider[] = [builtin],
  revision = 0,
  enabled = true,
): MetadataSettings {
  return { revision, enabled, providers: [...providers] };
}

export function catalog(index: number, name = `Personal catalog ${index}`) {
  const suffix = String(index).padStart(12, "0");
  const service: PluginService = {
    id: `10000000-0000-4000-8000-${suffix}`,
    name,
    base_url: `https://catalog-${index}.invalid/`,
    revision: 1,
  };
  const installation: PluginInstallation = {
    id: `20000000-0000-4000-8000-${suffix}`,
    service_id: service.id,
    revision: 1,
    checked_at: 1,
    credential_set: false,
    manifest: {
      id: `org.hito.metadata-preferences-${index}`,
      name,
      protocol_version: 1,
      capabilities: ["catalog"],
    },
  };
  return { service, installation };
}

export function connections(
  items: ReturnType<typeof catalog>[] = [],
): PluginSettings {
  return {
    credential_storage_ready: true,
    services: items.map((item) => item.service),
    installations: items.map((item) => item.installation),
  };
}

export function reference(item: ReturnType<typeof catalog>): MetadataProvider {
  return { kind: "http_plugin", installation_id: item.installation.id };
}

export function preferences(account = "alice") {
  return createElement(MetadataPreferences, { account });
}

export function held<T>(fallback: T) {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((done) => {
    resolve = done;
  });
  return {
    promise,
    resolve,
    finish: async () => {
      resolve(fallback);
      await promise;
    },
  };
}
