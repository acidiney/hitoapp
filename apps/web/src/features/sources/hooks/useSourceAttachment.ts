import type { Schema } from "../../../shared/schema";
import { useMemo, useState } from "react";
import { errorMessage } from "../../../shared/format";
import { HttpError } from "../../../shared/http";
import { log } from "../../../shared/log";
import { pluginsApi } from "../../plugins/api/pluginsApi";
import { sourceBindingsApi } from "../api/sourceBindingsApi";

import { useRequestScope } from "../../../shared/hooks/useRequestScope";

type Phase = "settings" | "search" | "review" | "attach";
type Work = {
  generation: number;
  controller: AbortController | null;
  phase: Phase | null;
};
type State = {
  loaded: boolean;
  connections: Schema<"PluginInstallation">[];
  names: Record<string, string>;
  choice: string | null;
  query: string;
  items: Schema<"ProviderRecording">[];
  review: {
    candidate: Schema<"ProviderCandidate">;
    detail: Schema<"ProviderSourceReview">;
  } | null;
  attached: boolean;
  pending: Phase | null;
  error: string;
  existing: string | null;
};
type Snapshot = State & { owner: Work };

const alreadyUsedMessage = "This video is already the source of another track.";

function failure(error: unknown) {
  if (error instanceof HttpError && error.code === "already_bound")
    return {
      error: alreadyUsedMessage,
      existing: error.recordingId ?? null,
    };
  return { error: errorMessage(error), existing: null };
}

function empty(owner: Work): Snapshot {
  return {
    owner,
    loaded: false,
    connections: [],
    names: {},
    choice: null,
    query: "",
    items: [],
    review: null,
    attached: false,
    pending: null,
    error: "",
    existing: null,
  };
}

export function useSourceAttachment(
  account: string,
  recording: string,
  confirmed: () => void,
) {
  const scope = useRequestScope(JSON.stringify([account, recording]));
  const owner = useMemo<Work>(
    () => ({ generation: 0, controller: null, phase: null }),
    [scope],
  );
  const [snapshot, setSnapshot] = useState(() => empty(owner));
  const current = snapshot.owner === owner ? snapshot : empty(owner);
  const connection =
    current.connections.find((item) => item.id === current.choice) ?? null;

  function cancel() {
    owner.controller?.abort();
    owner.controller = null;
    owner.phase = null;
    owner.generation++;
  }

  async function run<T>(
    phase: Phase,
    initial: Partial<State>,
    load: (signal: AbortSignal) => Promise<T>,
    publish: (result: T) => Partial<State>,
  ): Promise<boolean> {
    const request = scope.capture();
    if (!request.active() || owner.phase === "attach") return false;
    cancel();
    const controller = new AbortController();
    owner.controller = controller;
    owner.phase = phase;
    const generation = owner.generation;
    const signal = AbortSignal.any([request.signal, controller.signal]);
    const active = () =>
      request.active() && owner.generation === generation && !signal.aborted;
    const started = performance.now();
    let reason = "cancelled";
    setSnapshot({
      ...current,
      ...initial,
      owner,
      pending: phase,
      error: "",
      existing: null,
    });
    log.debug("source_attachment.started", {
      account_id: account,
      recording_id: recording,
      action: phase,
    });
    try {
      const result = await load(signal);
      if (!active()) return false;
      setSnapshot((value) =>
        active() && value.owner === owner
          ? { ...value, ...publish(result) }
          : value,
      );
      reason = "accepted";
      return true;
    } catch (error) {
      if (active()) {
        reason = "failed";
        setSnapshot((value) =>
          active() && value.owner === owner
            ? { ...value, ...failure(error) }
            : value,
        );
      }
      return false;
    } finally {
      if (owner.controller === controller) {
        owner.controller = null;
        owner.phase = null;
      }
      if (active())
        setSnapshot((value) =>
          active() && value.owner === owner
            ? { ...value, pending: null }
            : value,
        );
      const record =
        reason === "failed"
          ? log.warn
          : phase === "attach" && reason === "accepted"
            ? log.info
            : log.debug;
      record("source_attachment.finished", {
        account_id: account,
        recording_id: recording,
        action: phase,
        reason,
        duration_ms: Math.round(performance.now() - started),
      });
    }
  }

  async function open() {
    if (current.loaded || owner.phase) return false;
    return run(
      "settings",
      {},
      (signal) => pluginsApi.settings(account, signal),
      (settings) => {
        const connections = settings.installations.filter(
          (item) =>
            item.manifest.capabilities.includes("search") &&
            item.manifest.capabilities.includes("sources"),
        );
        return {
          loaded: true,
          connections,
          names: Object.fromEntries(
            settings.services.map((service) => [service.id, service.name]),
          ),
          choice: connections[0]?.id ?? null,
        };
      },
    );
  }

  function choose(id: string) {
    if (
      !scope.capture().active() ||
      owner.phase === "attach" ||
      id === current.choice ||
      !current.connections.some((item) => item.id === id)
    )
      return;
    cancel();
    setSnapshot({
      ...current,
      choice: id,
      items: [],
      query: "",
      review: null,
      attached: false,
      pending: null,
      error: "",
      existing: null,
    });
  }

  async function search(query: string) {
    if (!scope.capture().active() || !connection || owner.phase === "attach")
      return false;
    query = query.trim();
    if (!query) {
      cancel();
      setSnapshot({
        ...current,
        items: [],
        query: "",
        review: null,
        attached: false,
        pending: null,
        error: "",
        existing: null,
      });
      return true;
    }
    return run(
      "search",
      { query, items: [], review: null, attached: false },
      (signal) =>
        pluginsApi.search(
          account,
          connection.id,
          {
            installation_revision: connection.revision,
            query,
            cursor: null,
            limit: 20,
          },
          signal,
        ),
      (page) => ({ items: page.items }),
    );
  }

  async function review(hit: Schema<"ProviderRecording">) {
    if (
      !connection ||
      owner.phase ||
      !current.items.some((item) => item.id === hit.id)
    )
      return false;
    const candidate: Schema<"ProviderCandidate"> = {
      installation_id: connection.id,
      installation_revision: connection.revision,
      provider_recording_id: hit.id,
    };
    return run(
      "review",
      { review: null, attached: false },
      (signal) =>
        sourceBindingsApi.review(account, recording, candidate, signal),
      (detail) => ({ review: { candidate, detail } }),
    );
  }

  function dismissReview() {
    if (owner.phase === "attach" || !current.review) return;
    setSnapshot({ ...current, review: null });
  }

  async function confirm() {
    const request = scope.capture();
    const review = current.review;
    if (
      !request.active() ||
      !review ||
      !connection ||
      owner.phase ||
      connection.id !== review.candidate.installation_id ||
      connection.revision !== review.candidate.installation_revision
    )
      return false;
    const attached = await run(
      "attach",
      {},
      async (signal) => {
        const result = await sourceBindingsApi.attach(
          account,
          recording,
          {
            candidate: review.candidate,
            review_stamp: review.detail.review_stamp,
          },
          signal,
        );
        if (result.id !== recording)
          throw new Error("This source could not be added to the saved track.");
        return result;
      },
      () => ({ attached: true, review: null, items: [], query: "" }),
    );
    if (attached && request.active()) confirmed();
    return attached;
  }

  return {
    loaded: current.loaded,
    connections: current.connections,
    names: current.names,
    query: current.query,
    items: current.items,
    review: current.review,
    attached: current.attached,
    pending: current.pending,
    error: current.error,
    existing: current.existing,
    connection,
    open,
    choose,
    search,
    reviewCandidate: review,
    dismissReview,
    confirm,
  };
}
