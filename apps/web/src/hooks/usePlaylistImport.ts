import { useMemo, useState } from "react";
import { router } from "@inertiajs/react";
import { pluginsApi } from "../pluginsApi";
import { errorMessage } from "../format";
import { HttpError } from "../http";
import { log } from "../log";
import type { ImportProviderPlaylist, PluginInstallation } from "../types";
import { useRequestScope } from "./useRequestScope";

type ImportOwner = {
  attempt: Readonly<ImportProviderPlaylist> | null;
  generation: number;
  run: { active: () => boolean } | null;
};

export function usePlaylistImport(
  account: string,
  connection: PluginInstallation | null,
) {
  const scope = useRequestScope(
    JSON.stringify([account, connection?.id, connection?.revision]),
  );
  const owner = useMemo<ImportOwner>(
    () => ({ attempt: null, generation: 0, run: null }),
    [account, connection?.id],
  );
  const [state, setState] = useState<{
    owner: ImportOwner;
    scope: typeof scope;
    pending: boolean;
    error: string;
  } | null>(null);
  const current =
    state?.owner === owner && state.scope === scope ? state : null;

  async function perform(
    body: Readonly<ImportProviderPlaylist>,
    action: "copy" | "retry",
  ): Promise<boolean> {
    const request = scope.capture();
    if (
      owner.run?.active() ||
      !request.active() ||
      !connection?.manifest.capabilities.includes("playlists")
    )
      return false;
    const generation = ++owner.generation;
    const run = {
      active: () => request.active() && owner.generation === generation,
    };
    const started = performance.now();
    const fields = {
      account_id: account,
      operation_id: body.request_id,
      action,
    };
    owner.attempt = body;
    owner.run = run;
    setState({ owner, scope, pending: true, error: "" });
    log.info("playlist_import.started", fields);
    try {
      const receipt = await pluginsApi.importPlaylist(
        account,
        connection.id,
        body,
        request.signal,
      );
      if (!run.active()) return false;
      if (receipt.request_id !== body.request_id) {
        throw new Error("Hito returned a different import receipt. Try again.");
      }
      log.info("playlist_import.completed", {
        ...fields,
        playlist_id: receipt.playlist_id,
        duration_ms: Math.round(performance.now() - started),
      });
      owner.attempt = null;
      router.visit(`/playlists/${encodeURIComponent(receipt.playlist_id)}`);
      return true;
    } catch (error) {
      if (run.active()) {
        setState({ owner, scope, pending: false, error: errorMessage(error) });
        log.warn("playlist_import.failed", {
          ...fields,
          reason: error instanceof HttpError ? "http" : "request",
          status: error instanceof HttpError ? error.status : undefined,
          duration_ms: Math.round(performance.now() - started),
        });
      }
      return false;
    } finally {
      if (run.active()) {
        setState((value) =>
          value?.owner === owner && value.scope === scope
            ? { ...value, pending: false }
            : value,
        );
      } else {
        log.debug("playlist_import.cancelled", {
          ...fields,
          duration_ms: Math.round(performance.now() - started),
        });
      }
      if (owner.run === run) owner.run = null;
    }
  }

  async function importCopy(reference: string): Promise<boolean> {
    if (owner.run?.active() || !scope.capture().active() || !connection)
      return false;
    reference = reference.trim();
    if (
      !reference ||
      new TextEncoder().encode(reference).byteLength > 2048 ||
      /[\u0000-\u001f\u007f-\u009f]/u.test(reference)
    ) {
      setState({
        owner,
        scope,
        pending: false,
        error:
          "Use a playlist link or identifier of up to 2,048 bytes without control characters.",
      });
      log.warn("playlist_import.rejected", {
        account_id: account,
        reason: "invalid_reference",
      });
      return false;
    }
    return perform(
      {
        request_id: crypto.randomUUID(),
        installation_revision: connection.revision,
        playlist_reference: reference,
      },
      "copy",
    );
  }

  return {
    importCopy,
    retry: () =>
      owner.attempt ? perform(owner.attempt, "retry") : Promise.resolve(false),
    hasAttempt: owner.attempt !== null,
    pending: current?.pending ?? false,
    error: current?.error ?? "",
  };
}
