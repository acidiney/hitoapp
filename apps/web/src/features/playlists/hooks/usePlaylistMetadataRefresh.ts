import { useEffect, useMemo, useState } from "react";
import { pluginsApi } from "../../../pluginsApi";
import { playlistMetadataApi } from "../api/playlistMetadataApi";
import { HttpError } from "../../../shared/http";
import { createUuid } from "../../../shared/uuid";
import { log } from "../../../shared/log";
import type {
  ImportedPlaylistMetadataRefresh,
  PlaylistDetail,
  RefreshImportedPlaylistMetadata,
} from "../../../shared/types";
import { useRequestScope } from "../../../shared/hooks/useRequestScope";
import { usePlaylistMetadataDetail } from "./usePlaylistMetadataDetail";

type MetadataOwner = {
  run: AbortController | null;
  attempt: Readonly<RefreshImportedPlaylistMetadata> | null;
};
type MetadataState = {
  owner: MetadataOwner;
  pending: boolean;
  hasAttempt: boolean;
  receipt: ImportedPlaylistMetadataRefresh | null;
  error: string;
};

export function usePlaylistMetadataRefresh(
  account: string,
  playlist: string | null,
  origin: PlaylistDetail["origin"],
) {
  const key = JSON.stringify([account, playlist, origin]);
  const scope = useRequestScope(key);
  const detail = usePlaylistMetadataDetail(account, playlist, key);
  const owner = useMemo<MetadataOwner>(
    () => ({ run: null, attempt: null }),
    [key],
  );
  const [state, setState] = useState<MetadataState | null>(null);
  const current = state?.owner === owner ? state : null;

  useEffect(() => () => owner.run?.abort(), [owner]);

  async function perform(retry: boolean): Promise<void> {
    const request = scope.capture();
    if (
      !playlist ||
      !origin ||
      owner.run ||
      !request.active() ||
      (retry ? !owner.attempt : owner.attempt !== null)
    )
      return;
    detail.cancel();
    const run = new AbortController();
    const signal = AbortSignal.any([request.signal, run.signal]);
    const active = () =>
      request.active() && !signal.aborted && owner.run === run;
    owner.run = run;
    setState({
      owner,
      pending: true,
      hasAttempt: owner.attempt !== null,
      receipt: null,
      error: "",
    });
    const started = performance.now();
    const fields = {
      account_id: account,
      playlist_id: playlist,
      action: retry ? "retry" : "refresh",
    };
    log.info("playlist_metadata.refresh_started", fields);
    try {
      if (!owner.attempt) {
        const settings = await pluginsApi.settings(account, signal);
        if (!active()) return;
        const connection = settings.installations.find(
          (value) => value.id === origin.installation_id,
        );
        if (
          !connection ||
          connection.service_id !== origin.service_id ||
          connection.manifest.id !== origin.provider ||
          !connection.manifest.capabilities.includes("playlists") ||
          !Number.isSafeInteger(connection.revision) ||
          connection.revision <= 0 ||
          !settings.services.some((service) => service.id === origin.service_id)
        )
          throw new Error("The original playlist connection is unavailable");
        owner.attempt = Object.freeze({
          request_id: createUuid(),
          installation_revision: connection.revision,
        });
      }
      const body = owner.attempt;
      const receipt = await playlistMetadataApi.refresh(
        account,
        playlist,
        body,
        signal,
      );
      if (!active()) return;
      if (
        receipt.request_id !== body.request_id ||
        receipt.playlist_id !== playlist
      )
        throw new Error(
          "The playlist metadata result did not match the request",
        );
      owner.attempt = null;
      setState({
        owner,
        pending: false,
        hasAttempt: false,
        receipt,
        error: "",
      });
      log.info("playlist_metadata.refresh_accepted", {
        ...fields,
        operation_id: body.request_id,
        count: receipt.updated_recordings,
        duration_ms: Math.round(performance.now() - started),
      });
      detail.reload();
    } catch (error) {
      if (active()) {
        const rejected =
          owner.attempt !== null &&
          error instanceof HttpError &&
          error.status === 409 &&
          error.code === "operation_rejected";
        if (rejected) owner.attempt = null;
        setState({
          owner,
          pending: false,
          hasAttempt: owner.attempt !== null,
          receipt: null,
          error: rejected
            ? "This playlist or its connection changed. Check plugin settings, then find missing albums again."
            : owner.attempt
              ? "Hito could not confirm the album lookup. Retry the same request to recover its result."
              : "Hito could not check the original playlist connection. Check plugin settings and try again.",
        });
        log.warn("playlist_metadata.refresh_failed", {
          ...fields,
          reason: rejected
            ? "connection_changed"
            : error instanceof HttpError
              ? "http"
              : "request",
          status: error instanceof HttpError ? error.status : undefined,
          duration_ms: Math.round(performance.now() - started),
        });
      }
    } finally {
      if (owner.run === run) owner.run = null;
      if (!request.active() || signal.aborted)
        log.debug("playlist_metadata.refresh_cancelled", fields);
    }
  }

  return {
    pending: current?.pending ?? false,
    hasAttempt: current?.hasAttempt ?? false,
    receipt: current?.receipt ?? null,
    error: current?.error ?? "",
    detailError: detail.error,
    detailsPending: detail.pending,
    reloadDetails: detail.reload,
    refresh: () => perform(false),
    retry: () => perform(true),
  };
}
