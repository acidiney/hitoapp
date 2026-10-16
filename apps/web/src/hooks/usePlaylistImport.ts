import { useMemo, useState } from "react";
import { router } from "@inertiajs/react";
import { pluginsApi } from "../pluginsApi";
import { errorMessage } from "../format";
import { HttpError } from "../http";
import { log } from "../log";
import type { ImportProviderPlaylist, PluginInstallation } from "../types";
import { useRequestScope } from "./useRequestScope";

export function usePlaylistImport(
  account: string,
  connection: PluginInstallation | null,
) {
  const scope = useRequestScope(
    JSON.stringify([account, connection?.id, connection?.revision]),
  );
  const owner = useMemo(() => ({ busy: false }), [scope]);
  const [state, setState] = useState<{
    owner: typeof owner;
    pending: boolean;
    error: string;
  } | null>(null);
  const current = state?.owner === owner ? state : null;

  async function importCopy(reference: string): Promise<boolean> {
    const request = scope.capture();
    if (
      owner.busy ||
      !request.active() ||
      !connection?.manifest.capabilities.includes("playlists")
    )
      return false;
    reference = reference.trim();
    if (
      !reference ||
      new TextEncoder().encode(reference).byteLength > 2048 ||
      /[\u0000-\u001f\u007f-\u009f]/u.test(reference)
    ) {
      setState({
        owner,
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
    const body: ImportProviderPlaylist = {
      request_id: crypto.randomUUID(),
      installation_revision: connection.revision,
      playlist_reference: reference,
    };
    const started = performance.now();
    const fields = { account_id: account, operation_id: body.request_id };
    owner.busy = true;
    setState({ owner, pending: true, error: "" });
    log.info("playlist_import.started", fields);
    try {
      const receipt = await pluginsApi.importPlaylist(
        account,
        connection.id,
        body,
        request.signal,
      );
      if (!request.active()) return false;
      if (receipt.request_id !== body.request_id) {
        throw new Error("Hito returned a different import receipt. Try again.");
      }
      log.info("playlist_import.completed", {
        ...fields,
        playlist_id: receipt.playlist_id,
        duration_ms: Math.round(performance.now() - started),
      });
      router.visit(`/playlists/${encodeURIComponent(receipt.playlist_id)}`);
      return true;
    } catch (error) {
      if (request.active()) {
        setState({ owner, pending: false, error: errorMessage(error) });
        log.warn("playlist_import.failed", {
          ...fields,
          reason: error instanceof HttpError ? "http" : "request",
          status: error instanceof HttpError ? error.status : undefined,
          duration_ms: Math.round(performance.now() - started),
        });
      }
      return false;
    } finally {
      owner.busy = false;
      if (request.active()) {
        setState((value) =>
          value?.owner === owner ? { ...value, pending: false } : value,
        );
      } else {
        log.debug("playlist_import.cancelled", {
          ...fields,
          duration_ms: Math.round(performance.now() - started),
        });
      }
    }
  }

  return {
    importCopy,
    pending: current?.pending ?? false,
    error: current?.error ?? "",
  };
}
