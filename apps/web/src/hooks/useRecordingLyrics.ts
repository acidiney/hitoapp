import { useMemo, useState } from "react";
import { HttpError } from "../shared/http";
import { log } from "../shared/log";
import {
  lyricsApi,
  type LyricsLookup,
  type RecordingLyricsProvider,
} from "../lyricsApi";
import { pluginsApi } from "../pluginsApi";
import type { RecordingMetadata } from "../shared/types";
import { createUuid } from "../shared/uuid";
import { useRequestScope } from "../shared/hooks/useRequestScope";
import { useLyricProviderChoices } from "./useLyricProviderChoices";

type Target = Pick<
  RecordingLyricsProvider,
  "observation_id" | "installation_id" | "provider_recording"
> & { installation_revision: number | null };

type Owner = {
  generation: number;
  pending: boolean;
  controller: AbortController | null;
};
type State = {
  owner: Owner;
  pending: boolean;
  result: LyricsLookup | null;
  error: string;
  refresh: boolean;
};

function empty(owner: Owner): State {
  return { owner, pending: false, result: null, error: "", refresh: false };
}

export function useRecordingLyrics(account: string, detail: RecordingMetadata) {
  const providers = useLyricProviderChoices(account, detail);
  const selected = detail.selected_observation;
  const observation =
    selected?.origin === "provider_response" &&
    selected.plugin_installation_id &&
    selected.source_id === null
      ? selected
      : null;
  const target: Target | null = providers.engaged
    ? providers.selected
    : observation
      ? {
          observation_id: observation.id,
          installation_id: observation.plugin_installation_id!,
          installation_revision: null,
          provider_recording: {
            id: observation.provider_recording_id!,
            revision: observation.provider_revision,
          },
        }
      : null;
  const recording = detail.recording_id;
  const scope = useRequestScope(
    JSON.stringify([
      account,
      recording,
      target?.observation_id,
      target?.installation_id,
      target?.installation_revision,
      target?.provider_recording.id,
      target?.provider_recording.revision,
    ]),
  );
  const owner = useMemo<Owner>(
    () => ({ generation: 0, pending: false, controller: null }),
    [scope],
  );
  const [snapshot, setSnapshot] = useState(() => empty(owner));
  const current = snapshot.owner === owner ? snapshot : empty(owner);

  function cancel() {
    if (!scope.capture().active()) return;
    owner.generation++;
    owner.controller?.abort();
    owner.controller = null;
    owner.pending = false;
    setSnapshot(empty(owner));
  }

  function refreshProviders() {
    cancel();
    void providers.load();
  }

  async function load() {
    const request = scope.capture();
    if (!request.active() || owner.pending || !target) return;
    const generation = ++owner.generation;
    const controller = new AbortController();
    owner.controller = controller;
    owner.pending = true;
    const signal = AbortSignal.any([request.signal, controller.signal]);
    const live = () => request.active() && owner.generation === generation;
    const active = () => live() && !signal.aborted;
    const requestId = createUuid();
    const started = performance.now();
    let outcome = "cancelled";
    let expired = false;
    const timer = setTimeout(() => {
      if (!active()) return;
      expired = true;
      controller.abort();
      owner.pending = false;
      setSnapshot({
        ...empty(owner),
        error: "Lyrics took too long to load. Try again.",
      });
    }, 15_000);
    setSnapshot({ ...empty(owner), pending: true });
    log.debug("lyrics.started", { action: "read", request_id: requestId });
    try {
      let revision = target.installation_revision;
      if (revision === null) {
        const settings = await pluginsApi.settings(account, signal);
        if (!active()) return;
        const connection = settings.installations.find(
          (item) =>
            item.id === target.installation_id &&
            item.manifest.capabilities.includes("lyrics") &&
            settings.services.some((service) => service.id === item.service_id),
        );
        if (!connection) {
          outcome = "unsupported";
          setSnapshot({ ...empty(owner), result: { status: "unsupported" } });
          return;
        }
        revision = connection.revision;
      }
      const result = await lyricsApi.read(
        account,
        recording,
        {
          observation_id: target.observation_id,
          installation_revision: revision,
        },
        signal,
      );
      if (!active()) return;
      if (
        result.status === "ready" &&
        (result.recording_id !== recording ||
          result.observation_id !== target.observation_id ||
          result.document.recording.id !== target.provider_recording.id ||
          (result.document.recording.revision ?? null) !==
            (target.provider_recording.revision ?? null))
      ) {
        outcome = "changed";
        setSnapshot({
          ...empty(owner),
          refresh: true,
          error: "Track details changed. Refresh them before loading lyrics.",
        });
        return;
      }
      outcome =
        result.status === "ready" && !result.document.lyrics
          ? "not_found"
          : result.status;
      setSnapshot({ ...empty(owner), result });
    } catch (error) {
      if (!active()) return;
      outcome = "failed";
      const refresh =
        error instanceof HttpError && [404, 409].includes(error.status);
      const message = refresh
        ? "Track details or the provider connection changed. Refresh this track and try again."
        : error instanceof HttpError && [401, 403].includes(error.status)
          ? "Your account access changed. Sign in again to load lyrics."
          : "Lyrics could not be loaded. Try again.";
      setSnapshot({ ...empty(owner), error: message, refresh });
    } finally {
      clearTimeout(timer);
      if (owner.controller === controller) {
        owner.controller = null;
        owner.pending = false;
      }
      if (active())
        setSnapshot((value) =>
          active() && value.owner === owner
            ? { ...value, pending: false }
            : value,
        );
      log.debug("lyrics.finished", {
        action: "read",
        request_id: requestId,
        reason: expired ? "deadline" : outcome,
        duration_ms: Math.round(performance.now() - started),
      });
    }
  }

  return {
    providers,
    available: target !== null,
    pending: current.pending,
    result: current.result,
    error: current.error,
    refresh: current.refresh,
    refreshProviders,
    load,
    cancel,
  };
}
