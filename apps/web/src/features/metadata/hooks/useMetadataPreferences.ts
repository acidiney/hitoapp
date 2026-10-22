import { useEffect, useMemo, useState } from "react";
import { errorMessage } from "../../../shared/format";
import { log } from "../../../shared/log";
import { metadataPreferencesApi } from "../api/metadataPreferencesApi";
import { pluginsApi } from "../../plugins/api/pluginsApi";
import type {
  MetadataProvider,
  MetadataSettings,
  PluginSettings,
  UpdateMetadataSettings,
} from "../../../shared/types";
import { createUuid } from "../../../shared/uuid";
import { useRequestScope } from "../../../shared/hooks/useRequestScope";

const MAX_PROVIDERS = 8;
type Phase = "load" | "save" | null;
type Owner = { generation: number; pending: Phase };
type Snapshot = {
  owner: Owner;
  accepted: MetadataSettings | null;
  draft: UpdateMetadataSettings | null;
  connections: PluginSettings | null;
  pending: Phase;
  error: string;
  saved: boolean;
};

function empty(owner: Owner): Snapshot {
  return {
    owner,
    accepted: null,
    draft: null,
    connections: null,
    pending: "load",
    error: "",
    saved: false,
  };
}

function draft(settings: MetadataSettings): UpdateMetadataSettings {
  return { ...settings, providers: [...settings.providers] };
}

function providerKey(provider: MetadataProvider) {
  return provider.kind === "builtin_musicbrainz"
    ? provider.kind
    : provider.installation_id;
}

export function useMetadataPreferences(account: string) {
  const scope = useRequestScope(account);
  const owner = useMemo<Owner>(
    () => ({ generation: 0, pending: "load" }),
    [scope],
  );
  const [snapshot, setSnapshot] = useState(() => empty(owner));
  const [revision, setRevision] = useState(0);
  const current = snapshot.owner === owner ? snapshot : empty(owner);
  const dirty =
    current.accepted !== null &&
    current.draft !== null &&
    (current.accepted.enabled !== current.draft.enabled ||
      JSON.stringify(current.accepted.providers) !==
        JSON.stringify(current.draft.providers));

  useEffect(() => {
    const request = scope.capture();
    const controller = new AbortController();
    const signal = AbortSignal.any([request.signal, controller.signal]);
    const generation = ++owner.generation;
    owner.pending = "load";
    const active = () =>
      request.active() && !signal.aborted && owner.generation === generation;
    const requestId = createUuid();
    const started = performance.now();
    let reason = "cancelled";
    setSnapshot(empty(owner));
    log.debug("metadata_preferences.started", {
      action: "load",
      request_id: requestId,
    });
    void Promise.all([
      metadataPreferencesApi.settings(account, signal),
      pluginsApi.settings(account, signal),
    ])
      .then(([accepted, connections]) => {
        if (!active()) return;
        reason = "accepted";
        owner.pending = null;
        setSnapshot({
          owner,
          accepted,
          draft: draft(accepted),
          connections,
          pending: null,
          error: "",
          saved: false,
        });
      })
      .catch((error: unknown) => {
        if (!active()) return;
        reason = "failed";
        owner.pending = null;
        controller.abort();
        setSnapshot({
          ...empty(owner),
          pending: null,
          error: errorMessage(error),
        });
      })
      .finally(() => {
        log.debug("metadata_preferences.finished", {
          action: "load",
          request_id: requestId,
          reason,
          duration_ms: Math.round(performance.now() - started),
        });
      });
    return () => controller.abort();
  }, [account, owner, scope, revision]);

  function setEnabled(enabled: boolean) {
    if (!scope.capture().active() || owner.pending !== null) return;
    setSnapshot((value) =>
      value.owner === owner && value.draft !== null
        ? {
            ...value,
            draft: { ...value.draft, enabled },
            error: "",
            saved: false,
          }
        : value,
    );
  }

  function editProviders(
    change: (providers: MetadataProvider[]) => MetadataProvider[],
  ) {
    if (!scope.capture().active() || owner.pending !== null) return;
    setSnapshot((value) => {
      if (value.owner !== owner || value.draft === null) return value;
      const providers = change(value.draft.providers);
      if (
        providers === value.draft.providers ||
        providers.length > MAX_PROVIDERS ||
        new Set(providers.map(providerKey)).size !== providers.length
      )
        return value;
      return {
        ...value,
        draft: { ...value.draft, providers },
        error: "",
        saved: false,
      };
    });
  }

  function addProvider(provider: MetadataProvider) {
    if (provider.kind === "http_plugin") {
      const installation = current.connections?.installations.find(
        (item) => item.id === provider.installation_id,
      );
      if (
        !installation?.manifest.capabilities.includes("catalog") ||
        !current.connections?.services.some(
          (service) => service.id === installation.service_id,
        )
      )
        return;
    }
    editProviders((providers) =>
      providers.some((item) => providerKey(item) === providerKey(provider))
        ? providers
        : [...providers, provider],
    );
  }

  function removeProvider(provider: MetadataProvider) {
    editProviders((providers) =>
      providers.filter((item) => providerKey(item) !== providerKey(provider)),
    );
  }

  function moveProvider(provider: MetadataProvider, direction: -1 | 1) {
    editProviders((providers) => {
      const from = providers.findIndex(
        (item) => providerKey(item) === providerKey(provider),
      );
      const to = from + direction;
      if (from < 0 || to < 0 || to >= providers.length) return providers;
      const moved = [...providers];
      [moved[from], moved[to]] = [moved[to], moved[from]];
      return moved;
    });
  }

  async function save() {
    const request = scope.capture();
    if (!request.active() || !dirty || !current.draft || owner.pending !== null)
      return false;
    owner.pending = "save";
    const generation = ++owner.generation;
    const active = () => request.active() && owner.generation === generation;
    const requestId = createUuid();
    const started = performance.now();
    let reason = "cancelled";
    setSnapshot((value) =>
      value.owner === owner
        ? { ...value, pending: "save", error: "", saved: false }
        : value,
    );
    log.debug("metadata_preferences.started", {
      action: "save",
      request_id: requestId,
    });
    try {
      const accepted = await metadataPreferencesApi.update(
        account,
        current.draft,
        request.signal,
      );
      if (!active()) return false;
      reason = "accepted";
      owner.pending = null;
      setSnapshot((value) =>
        value.owner === owner
          ? {
              ...value,
              accepted,
              draft: draft(accepted),
              pending: null,
              error: "",
              saved: true,
            }
          : value,
      );
      return true;
    } catch (error) {
      if (active()) {
        reason = "failed";
        owner.pending = null;
        setSnapshot((value) =>
          value.owner === owner
            ? {
                ...value,
                pending: null,
                error: errorMessage(error),
                saved: false,
              }
            : value,
        );
      }
      return false;
    } finally {
      const record =
        reason === "accepted"
          ? log.info
          : reason === "failed"
            ? log.warn
            : log.debug;
      record("metadata_preferences.finished", {
        action: "save",
        request_id: requestId,
        reason,
        duration_ms: Math.round(performance.now() - started),
      });
    }
  }

  return {
    loaded: current.accepted !== null,
    enabled: current.draft?.enabled ?? false,
    providers: current.draft?.providers ?? [],
    connections: current.connections,
    pending: current.pending !== null,
    error: current.error,
    saved: current.saved,
    dirty,
    canAddProvider: (current.draft?.providers.length ?? 0) < MAX_PROVIDERS,
    addProvider,
    removeProvider,
    moveProvider,
    setEnabled,
    save,
    reload: () => {
      if (!scope.capture().active() || owner.pending !== null) return;
      owner.pending = "load";
      owner.generation++;
      setSnapshot((value) =>
        value.owner === owner
          ? { ...value, pending: "load", error: "", saved: false }
          : value,
      );
      setRevision((value) => value + 1);
    },
  };
}
