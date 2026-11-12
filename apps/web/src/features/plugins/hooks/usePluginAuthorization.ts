import type { Schema } from "../../../shared/schema";
import { useCallback, useEffect, useState } from "react";
import { pluginsApi } from "../api/pluginsApi";
import { errorMessage } from "../../../shared/format";
import { HttpError } from "../../../shared/http";
import { log } from "../../../shared/log";
import { onInvalidated } from "../../../shared/invalidation";
import { useRequestScope } from "../../../shared/hooks/useRequestScope";

const firstDelayMs = 1000;
const longestDelayMs = 5000;
const backoff = 1.5;

type Waiting = {
  id: string;
  url: string;
  expiresAt: number;
  opened: boolean;
};
type Phase =
  | { kind: "idle" }
  | { kind: "starting" }
  | { kind: "waiting"; attempt: Waiting }
  | { kind: "disconnecting" };
type Notice = { tone: "error" | "info"; text: string };
type State = {
  owner: string;
  connection: Schema<"PluginConnectionStatus"> | null;
  checkFailed: boolean;
  phase: Phase;
  notice: Notice | null;
};

const initial = (owner: string): State => ({
  owner,
  connection: null,
  checkFailed: false,
  phase: { kind: "idle" },
  notice: null,
});

export function usePluginAuthorization(
  account: string,
  plugin: Schema<"PluginInstallation">,
  desktop: boolean,
) {
  const enabled = plugin.manifest.authorization != null;
  const { id, revision } = plugin;
  const provider = plugin.manifest.name;
  const owner = `${account}/${id}`;
  const scope = useRequestScope(owner);
  const [stored, setStored] = useState(() => initial(owner));
  const state = stored.owner === owner ? stored : initial(owner);
  const update = useCallback(
    (patch: Partial<State>) =>
      setStored((current) => ({
        ...(current.owner === owner ? current : initial(owner)),
        ...patch,
      })),
    [owner],
  );
  const [reload, setReload] = useState(0);

  useEffect(() => {
    if (!enabled) return;
    const request = scope.capture();
    void pluginsApi
      .connectionStatus(account, id, request.signal)
      .then((connection) => {
        if (request.active()) update({ connection, checkFailed: false });
      })
      .catch(() => {
        if (!request.active()) return;
        log.warn("plugins.connection_check_failed", { account_id: account });
        update({ checkFailed: true });
      });
  }, [account, enabled, id, revision, reload, scope, update]);

  useEffect(
    () =>
      onInvalidated("plugins", (changed) => {
        if (changed === id || changed === null) setReload((value) => value + 1);
      }),
    [id],
  );

  const attempt = state.phase.kind === "waiting" ? state.phase.attempt : null;
  useEffect(() => {
    if (!attempt) return;
    const controller = new AbortController();
    let timer: ReturnType<typeof setTimeout> | undefined;
    let delay = firstDelayMs;
    let inflight = false;
    const end = (notice: Notice | null) =>
      update({ phase: { kind: "idle" }, notice });
    const schedule = () => {
      timer = setTimeout(() => void check(), delay);
      delay = Math.min(delay * backoff, longestDelayMs);
    };
    async function check() {
      if (inflight || controller.signal.aborted) return;
      inflight = true;
      clearTimeout(timer);
      try {
        const result = await pluginsApi.authorizationStatus(
          account,
          id,
          attempt!.id,
          controller.signal,
        );
        if (controller.signal.aborted) return;
        if (result.status === "completed") {
          log.info("plugins.authorization_completed", { account_id: account });
          end(null);
          setReload((value) => value + 1);
          return;
        }
        if (result.status === "failed") {
          log.info("plugins.authorization_failed", { account_id: account });
          end({
            tone: "error",
            text: `Signing in with ${provider} did not finish. Try again.`,
          });
          return;
        }
      } catch (error) {
        if (controller.signal.aborted) return;
        if (error instanceof HttpError && error.status === 404) {
          end({
            tone: "error",
            text: `Signing in with ${provider} could not be completed. Try again.`,
          });
          return;
        }
        log.debug("plugins.authorization_poll_failed", { account_id: account });
      } finally {
        inflight = false;
      }
      if (Date.now() >= attempt!.expiresAt * 1000) {
        log.info("plugins.authorization_expired", { account_id: account });
        end({
          tone: "error",
          text: `Signing in with ${provider} timed out. Try again.`,
        });
        return;
      }
      schedule();
    }
    const wake = () => {
      if (document.visibilityState !== "hidden") void check();
    };
    schedule();
    window.addEventListener("focus", wake);
    document.addEventListener("visibilitychange", wake);
    return () => {
      controller.abort();
      clearTimeout(timer);
      window.removeEventListener("focus", wake);
      document.removeEventListener("visibilitychange", wake);
    };
  }, [account, attempt, id, provider, update]);

  const connect = useCallback(async () => {
    const request = scope.capture();
    if (!request.active() || state.phase.kind !== "idle") return;
    const tab = desktop ? null : window.open("", "_blank");
    if (tab) tab.opener = null;
    update({ phase: { kind: "starting" }, notice: null });
    try {
      const begun = await pluginsApi.beginAuthorization(
        account,
        plugin,
        request.signal,
      );
      if (!request.active()) {
        tab?.close();
        return;
      }
      if (desktop) {
        await pluginsApi.launchAuthorization(
          account,
          id,
          begun.id,
          begun.authorization_url,
          request.signal,
        );
        if (!request.active()) return;
      }
      const opened = desktop || (tab !== null && !tab.closed);
      if (tab && opened) tab.location.href = begun.authorization_url;
      log.info("plugins.authorization_started", {
        account_id: account,
        count: opened ? 1 : 0,
      });
      update({
        phase: {
          kind: "waiting",
          attempt: {
            id: begun.id,
            url: begun.authorization_url,
            expiresAt: begun.expires_at,
            opened,
          },
        },
      });
    } catch (error) {
      tab?.close();
      if (request.active())
        update({
          phase: { kind: "idle" },
          notice: { tone: "error", text: errorMessage(error) },
        });
    }
  }, [account, desktop, id, plugin, scope, state.phase.kind, update]);

  const disconnect = useCallback(async () => {
    const request = scope.capture();
    if (!request.active() || state.phase.kind !== "idle") return false;
    update({ phase: { kind: "disconnecting" }, notice: null });
    try {
      await pluginsApi.disconnect(account, id, request.signal);
      if (!request.active()) return false;
      log.info("plugins.disconnected", { account_id: account });
      update({ phase: { kind: "idle" } });
      setReload((value) => value + 1);
      return true;
    } catch (error) {
      if (request.active())
        update({
          phase: { kind: "idle" },
          notice: { tone: "error", text: errorMessage(error) },
        });
      return false;
    }
  }, [account, id, scope, state.phase.kind, update]);

  const stopWaiting = useCallback(
    () => update({ phase: { kind: "idle" } }),
    [update],
  );
  const check = useCallback(() => {
    update({ checkFailed: false });
    setReload((value) => value + 1);
  }, [update]);

  return {
    enabled,
    connection: state.connection,
    checkFailed: state.checkFailed,
    phase: state.phase,
    notice: state.notice,
    connect,
    disconnect,
    stopWaiting,
    check,
  };
}
export type PluginAuthorizationActions = ReturnType<
  typeof usePluginAuthorization
>;
