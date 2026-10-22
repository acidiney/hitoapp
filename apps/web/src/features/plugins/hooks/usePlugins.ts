import { useEffect, useMemo, useRef, useState } from "react";
import { pluginsApi } from "../api/pluginsApi";
import type {
  InstallPlugin,
  OfferPluginService,
  PluginInstallation,
  PluginService,
  PluginSettings,
  PreviewPluginInstallation,
  RefreshPlugin,
} from "../../../shared/types";
import { useRequestScope } from "../../../shared/hooks/useRequestScope";
import { errorMessage } from "../../../shared/format";
import { log } from "../../../shared/log";
import { useRefreshArtworkAvailability } from "../../artwork/components/ArtworkProvider";

export function usePlugins(account: string) {
  const scope = useRequestScope(account);
  const refreshArtworkAvailability = useRefreshArtworkAvailability(account);
  const owner = useMemo(() => ({ scope, generation: 0 }), [scope]);
  const busy = useRef<typeof owner | null>(null);
  const [snapshot, setSnapshot] = useState<{
    owner: typeof owner;
    data: PluginSettings;
  } | null>(null);
  const [failure, setFailure] = useState<{
    owner: typeof owner;
    action: string;
    message: string;
  } | null>(null);
  const [pending, setPending] = useState<typeof owner | null>(null);
  const [revision, reload] = useState(0);
  useEffect(() => {
    const request = scope.capture();
    const controller = new AbortController();
    const signal = AbortSignal.any([request.signal, controller.signal]);
    const generation = owner.generation;
    void pluginsApi
      .settings(account, signal)
      .then((data) => {
        if (
          request.active() &&
          !signal.aborted &&
          owner.generation === generation &&
          busy.current !== owner
        ) {
          setSnapshot({ owner, data });
          setFailure(null);
        }
      })
      .catch((error) => {
        if (
          request.active() &&
          !signal.aborted &&
          owner.generation === generation
        )
          setFailure({ owner, action: "list", message: errorMessage(error) });
      });
    return () => controller.abort();
  }, [account, owner, scope, revision]);

  async function perform<T>(
    action: string,
    operation: (signal: AbortSignal) => Promise<T>,
    update: (data: PluginSettings, result: T) => PluginSettings,
    callerSignal?: AbortSignal,
  ) {
    if (busy.current === owner) return null;
    const request = scope.capture();
    const signal = callerSignal
      ? AbortSignal.any([request.signal, callerSignal])
      : request.signal;
    const active = () => request.active() && !signal.aborted;
    if (!active()) return null;
    busy.current = owner;
    owner.generation++;
    setPending(owner);
    setFailure(null);
    try {
      const result = await operation(signal);
      if (!active()) return null;
      setSnapshot((current) =>
        current?.owner === owner
          ? { owner, data: update(current.data, result) }
          : current,
      );
      log.info("plugins.completed", { account_id: account, action });
      return result;
    } catch (error) {
      if (active()) setFailure({ owner, action, message: errorMessage(error) });
      return null;
    } finally {
      if (busy.current === owner) busy.current = null;
      if (request.active()) setPending(null);
    }
  }
  const replace = (data: PluginSettings, installation: PluginInstallation) => ({
    ...data,
    installations: data.installations.map((item) =>
      item.id === installation.id ? installation : item,
    ),
  });
  return {
    data: snapshot?.owner === owner ? snapshot.data : null,
    error: failure?.owner === owner ? failure.message : "",
    pending: pending === owner,
    reload: () => {
      if (busy.current !== owner) reload((value) => value + 1);
    },
    preview: async (body: PreviewPluginInstallation, signal: AbortSignal) => {
      const request = scope.capture();
      const combined = AbortSignal.any([request.signal, signal]);
      if (!request.active() || combined.aborted) return null;
      setFailure((current) =>
        current?.owner === owner && current.action === "install"
          ? null
          : current,
      );
      const result = await pluginsApi.preview(account, body, combined);
      return request.active() && !combined.aborted ? result : null;
    },
    install: (body: InstallPlugin, signal?: AbortSignal) =>
      perform(
        "install",
        (signal) => pluginsApi.install(account, body, signal),
        (data, result) => ({
          ...data,
          installations: [...data.installations, result],
        }),
        signal,
      ),
    offer: (body: OfferPluginService) =>
      perform(
        "offer",
        (signal) => pluginsApi.offer(account, body, signal),
        (data, result) => ({ ...data, services: [...data.services, result] }),
      ),
    withdraw: (service: PluginService) =>
      perform(
        "withdraw",
        async (signal) => {
          await pluginsApi.withdraw(account, service, signal);
          return true;
        },
        (data) => ({
          ...data,
          services: data.services.filter((item) => item.id !== service.id),
          installations: data.installations.filter(
            (item) => item.service_id !== service.id,
          ),
        }),
      ),
    remove: (plugin: PluginInstallation) =>
      perform(
        "remove",
        async (signal) => {
          await pluginsApi.remove(account, plugin, signal);
          return true;
        },
        (data) => ({
          ...data,
          installations: data.installations.filter(
            (item) => item.id !== plugin.id,
          ),
        }),
      ),
    clearCredential: (plugin: PluginInstallation) =>
      perform(
        "clear_credential",
        (signal) => pluginsApi.clearCredential(account, plugin, signal),
        replace,
      ),
    refresh: async (plugin: PluginInstallation, body: RefreshPlugin) => {
      const request = scope.capture();
      const result = await perform(
        "refresh",
        (signal) => pluginsApi.refresh(account, plugin, body, signal),
        (data, result) =>
          result.kind === "updated" ? replace(data, result.installation) : data,
      );
      if (
        request.active() &&
        body.accept_manifest_digest != null &&
        result?.kind === "updated" &&
        !plugin.manifest.capabilities.includes("artwork") &&
        result.installation.manifest.capabilities.includes("artwork")
      )
        refreshArtworkAvailability();
      return result;
    },
  };
}
export type PluginActions = ReturnType<typeof usePlugins>;
