import { useMemo, useState } from "react";
import { router } from "@inertiajs/react";
import { pluginsApi } from "../pluginsApi";
import { errorMessage } from "../format";
import { HttpError } from "../http";
import { log } from "../log";
import type {
  FollowProviderPlaylist,
  ImportProviderPlaylist,
  PlaylistFollowInterval,
  PluginInstallation,
} from "../types";
import { useRequestScope } from "./useRequestScope";

type PlaylistAttempt =
  | { kind: "copy"; body: Readonly<ImportProviderPlaylist> }
  | { kind: "follow"; body: Readonly<FollowProviderPlaylist> };

export type PlaylistCreationMode = PlaylistAttempt["kind"];

type PlaylistOwner = {
  attempt: PlaylistAttempt | null;
  generation: number;
  run: { active: () => boolean } | null;
};

export function useProviderPlaylist(
  account: string,
  connection: PluginInstallation | null,
) {
  const scope = useRequestScope(
    JSON.stringify([account, connection?.id, connection?.revision]),
  );
  const owner = useMemo<PlaylistOwner>(
    () => ({ attempt: null, generation: 0, run: null }),
    [account, connection?.id],
  );
  const [state, setState] = useState<{
    owner: PlaylistOwner;
    scope: typeof scope;
    pending: boolean;
    error: string;
  } | null>(null);
  const current =
    state?.owner === owner && state.scope === scope ? state : null;

  async function perform(
    attempt: PlaylistAttempt,
    action: PlaylistCreationMode | "retry",
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
      operation_id: attempt.body.request_id,
      action,
    };
    const workflow =
      attempt.kind === "copy" ? "playlist_import" : "playlist_follow";
    owner.attempt = attempt;
    owner.run = run;
    setState({ owner, scope, pending: true, error: "" });
    log.info(`${workflow}.started`, fields);
    try {
      const receipt =
        attempt.kind === "copy"
          ? await pluginsApi.importPlaylist(
              account,
              connection.id,
              attempt.body,
              request.signal,
            )
          : await pluginsApi.followPlaylist(
              account,
              connection.id,
              attempt.body,
              request.signal,
            );
      if (!run.active()) return false;
      if (receipt.request_id !== attempt.body.request_id) {
        throw new Error(
          `Hito returned a different ${attempt.kind === "copy" ? "import" : "follow"} receipt. Try again.`,
        );
      }
      log.info(`${workflow}.completed`, {
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
        log.warn(`${workflow}.failed`, {
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
        log.debug(`${workflow}.cancelled`, {
          ...fields,
          duration_ms: Math.round(performance.now() - started),
        });
      }
      if (owner.run === run) owner.run = null;
    }
  }

  async function create(
    reference: string,
    kind: PlaylistCreationMode,
    interval: PlaylistFollowInterval,
  ): Promise<boolean> {
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
      log.warn(
        `${kind === "copy" ? "playlist_import" : "playlist_follow"}.rejected`,
        { account_id: account, reason: "invalid_reference" },
      );
      return false;
    }
    const body = {
      request_id: crypto.randomUUID(),
      installation_revision: connection.revision,
      playlist_reference: reference,
    };
    return perform(
      kind === "copy" ? { kind, body } : { kind, body: { ...body, interval } },
      kind,
    );
  }

  return {
    create,
    retry: () =>
      owner.attempt ? perform(owner.attempt, "retry") : Promise.resolve(false),
    hasAttempt: owner.attempt !== null,
    attemptKind: owner.attempt?.kind ?? null,
    pendingKind: current?.pending ? (owner.attempt?.kind ?? null) : null,
    pending: current?.pending ?? false,
    error: current?.error ?? "",
  };
}
