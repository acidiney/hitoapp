import { useMemo, useState } from "react";
import { HttpError } from "../http";
import { log } from "../log";
import { lyricsApi, type RecordingLyricsProvider } from "../lyricsApi";
import type { RecordingMetadata } from "../types";
import { createUuid } from "../uuid";
import { useRequestScope } from "./useRequestScope";

type Owner = {
  generation: number;
  pending: boolean;
  controller: AbortController | null;
};
type State = {
  owner: Owner;
  engaged: boolean;
  pending: boolean;
  items: RecordingLyricsProvider[] | null;
  selection: string | null;
  error: string;
  refresh: boolean;
};

function empty(owner: Owner): State {
  return {
    owner,
    engaged: false,
    pending: false,
    items: null,
    selection: null,
    error: "",
    refresh: false,
  };
}

export function useLyricProviderChoices(
  account: string,
  detail: RecordingMetadata,
) {
  const primary = detail.selected_observation;
  const recording = detail.recording_id;
  const scope = useRequestScope(
    JSON.stringify([
      account,
      recording,
      primary?.id,
      primary?.plugin_installation_id,
      primary?.provider_recording_id,
      primary?.provider_revision,
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
    setSnapshot({ ...empty(owner), engaged: true });
  }

  function select(observation: string) {
    if (
      !scope.capture().active() ||
      owner.pending ||
      !current.items?.some((item) => item.observation_id === observation)
    )
      return;
    setSnapshot({ ...current, selection: observation });
  }

  async function load() {
    const request = scope.capture();
    if (!request.active() || owner.pending) return;
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
        engaged: true,
        error: "Lyric providers took too long to load. Try again.",
      });
    }, 5_000);
    setSnapshot({ ...empty(owner), engaged: true, pending: true });
    log.debug("lyrics.providers.started", {
      action: "list",
      request_id: requestId,
    });
    try {
      const result = await lyricsApi.providers(account, recording, signal);
      if (!active()) return;
      if (result.recording_id !== recording) {
        outcome = "changed";
        setSnapshot({
          ...empty(owner),
          engaged: true,
          refresh: true,
          error:
            "Track details changed. Refresh them before choosing a provider.",
        });
        return;
      }
      outcome = "read";
      const selected =
        result.items.find(
          (item) => item.observation_id === current.selection,
        ) ??
        result.items.find((item) => item.observation_id === primary?.id) ??
        result.items[0];
      setSnapshot({
        ...empty(owner),
        engaged: true,
        items: result.items,
        selection: selected?.observation_id ?? null,
      });
    } catch (error) {
      if (!active()) return;
      outcome = "failed";
      const refresh =
        error instanceof HttpError && [404, 409].includes(error.status);
      setSnapshot({
        ...empty(owner),
        engaged: true,
        refresh,
        error: refresh
          ? "Track details or provider connections changed. Refresh this track and try again."
          : error instanceof HttpError && [401, 403].includes(error.status)
            ? "Your account access changed. Sign in again to choose a lyric provider."
            : "Lyric providers could not be loaded. Try again.",
      });
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
      log.debug("lyrics.providers.finished", {
        action: "list",
        request_id: requestId,
        reason: expired ? "deadline" : outcome,
        duration_ms: Math.round(performance.now() - started),
      });
    }
  }

  return {
    engaged: current.engaged,
    pending: current.pending,
    items: current.items,
    selected:
      current.items?.find(
        (item) => item.observation_id === current.selection,
      ) ?? null,
    error: current.error,
    refresh: current.refresh,
    load,
    select,
    cancel,
  };
}
