import { useState } from "react";
import { log } from "../../../shared/log";
import { createUuid } from "../../../shared/uuid";
import { lyricsApi } from "../api/lyricsApi";
import {
  changedFailure,
  deadlineFailure,
  lyricFailure,
  type LyricFailure,
} from "../model/lyricFailure";
import {
  firstSelectable,
  identityKey,
  type LyricIdentity,
  type LyricService,
} from "../model/lyricIdentity";
import { useLatestRun } from "./useLatestRun";

type Origin = "default" | "user";
type State = {
  owner: object;
  loading: boolean;
  loaded: boolean;
  services: LyricService[] | null;
  servicesFailure: LyricFailure | null;
  identities: LyricIdentity[] | null;
  failure: LyricFailure | null;
  selectedKey: string | null;
  origin: Origin;
};

function empty(owner: object): State {
  return {
    owner,
    loading: false,
    loaded: false,
    services: null,
    servicesFailure: null,
    identities: null,
    failure: null,
    selectedKey: null,
    origin: "default",
  };
}

type LoadOptions = { prefer?: string | null; byUser?: boolean };

export function useLyricIdentities(
  account: string,
  recording: string,
  onAccessLost: () => void,
) {
  const run = useLatestRun(JSON.stringify([account, recording]));
  const [snapshot, setSnapshot] = useState(() => empty(run.owner));
  const current = snapshot.owner === run.owner ? snapshot : empty(run.owner);

  function update(change: (value: State) => State) {
    setSnapshot((value) =>
      value.owner === run.owner ? change(value) : change(empty(run.owner)),
    );
  }

  function reset() {
    run.cancel();
    setSnapshot(empty(run.owner));
  }

  function select(key: string) {
    const target = current.identities?.find(
      (identity) => identityKey(identity) === key,
    );
    if (!target?.selectable) return;
    update((value) => ({ ...value, selectedKey: key, origin: "user" }));
  }

  async function load(options: LoadOptions = {}) {
    const started = performance.now();
    const requestId = createUuid();
    const active = run.start(5_000, () =>
      update((value) => ({
        ...value,
        loading: false,
        failure: deadlineFailure("identities"),
      })),
    );
    if (!active) return;
    let outcome = "cancelled";
    update((value) => ({ ...value, loading: true, failure: null }));
    log.debug("lyrics.identities.started", {
      action: "list",
      request_id: requestId,
    });
    try {
      const [services, identities] = await Promise.allSettled([
        lyricsApi.services(account, recording, active.signal),
        lyricsApi.identities(account, recording, active.signal),
      ]);
      if (!active.active()) return;
      const servicesFailure =
        services.status === "rejected"
          ? lyricFailure(services.reason, "services")
          : services.value.recording_id !== recording
            ? changedFailure()
            : null;
      const identitiesFailure =
        identities.status === "rejected"
          ? lyricFailure(identities.reason, "identities")
          : identities.value.recording_id !== recording
            ? changedFailure()
            : null;
      const failure = [identitiesFailure, servicesFailure].find(
        (item) => item?.kind === "access",
      );
      if (failure) {
        outcome = "access";
        setSnapshot({ ...empty(run.owner), loaded: true, failure });
        onAccessLost();
        return;
      }
      const items =
        identities.status === "fulfilled" && !identitiesFailure
          ? identities.value.items
          : null;
      outcome = identitiesFailure ? "failed" : "read";
      update((value) => {
        const wanted = options.prefer ?? value.selectedKey;
        const kept = items?.find(
          (identity) => identityKey(identity) === wanted && identity.selectable,
        );
        const chosen = kept ?? (items ? firstSelectable(items) : null);
        return {
          ...value,
          loading: false,
          loaded: true,
          services:
            services.status === "fulfilled" && !servicesFailure
              ? services.value.items
              : null,
          servicesFailure,
          identities: items,
          failure: identitiesFailure,
          selectedKey: chosen ? identityKey(chosen) : null,
          origin: options.byUser ? "user" : kept ? value.origin : "default",
        };
      });
    } finally {
      active.finish();
      if (active.active()) update((value) => ({ ...value, loading: false }));
      log.debug("lyrics.identities.finished", {
        action: "list",
        request_id: requestId,
        reason: outcome,
        duration_ms: Math.round(performance.now() - started),
      });
    }
  }

  const selected =
    current.identities?.find(
      (identity) => identityKey(identity) === current.selectedKey,
    ) ?? null;

  return {
    loading: current.loading,
    loaded: current.loaded,
    services: current.services,
    servicesFailure: current.servicesFailure,
    identities: current.identities,
    failure: current.failure,
    selected,
    origin: current.origin,
    load,
    select,
    reset,
  };
}
