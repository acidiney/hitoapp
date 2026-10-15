import { useEffect, useMemo, useRef, useState } from "react";
import { pluginsApi } from "../pluginsApi";
import type {
  InstallPlugin,
  OfferPluginService,
  PluginInstallation,
  PluginService,
  PluginSettings,
  RefreshPlugin,
} from "../types";
import { useRequestScope } from "./useRequestScope";
import { errorMessage } from "../format";
import { log } from "../log";

export function usePlugins(account: string) {
  const scope = useRequestScope(account);
  const owner = useMemo(() => ({ scope, generation: 0 }), [scope]);
  const busy = useRef<typeof owner | null>(null);
  const [snapshot, setSnapshot] = useState<{
    owner: typeof owner;
    data: PluginSettings;
  } | null>(null);
  const [failure, setFailure] = useState<{
    owner: typeof owner;
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
          setFailure({ owner, message: errorMessage(error) });
      });
    return () => controller.abort();
  }, [account, owner, scope, revision]);

  async function perform<T>(
    action: string,
    operation: (signal: AbortSignal) => Promise<T>,
    update: (data: PluginSettings, result: T) => PluginSettings,
  ) {
    if (busy.current === owner) return null;
    const request = scope.capture();
    busy.current = owner;
    owner.generation++;
    setPending(owner);
    setFailure(null);
    try {
      const result = await operation(request.signal);
      if (!request.active()) return null;
      setSnapshot((current) =>
        current?.owner === owner
          ? { owner, data: update(current.data, result) }
          : current,
      );
      log.info("plugins.changed", { account_id: account, action });
      return result;
    } catch (error) {
      if (request.active()) setFailure({ owner, message: errorMessage(error) });
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
    install: (body: InstallPlugin) =>
      perform(
        "install",
        (signal) => pluginsApi.install(account, body, signal),
        (data, result) => ({
          ...data,
          installations: [...data.installations, result],
        }),
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
    refresh: (plugin: PluginInstallation, body: RefreshPlugin) =>
      perform(
        "refresh",
        (signal) => pluginsApi.refresh(account, plugin, body, signal),
        (data, result) =>
          result.kind === "updated" ? replace(data, result.installation) : data,
      ),
  };
}
export type PluginActions = ReturnType<typeof usePlugins>;
