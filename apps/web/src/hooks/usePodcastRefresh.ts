import { useEffect, useMemo, useRef, useState } from "react";
import { HttpError } from "../http";
import { log } from "../log";
import { podcastApi } from "../podcastApi";
import type { PodcastSubscription } from "../types";
import { usePodcastCommand } from "./usePodcastCommand";
import { useRequestScope } from "./useRequestScope";

type EpisodeUpdates = {
  generation: object;
  knownRevision: number;
  pause: () => void;
  applyRevision: (revision: number) => Promise<void>;
};
type Owner = {
  subscription: PodcastSubscription;
  highestRevision: number;
  awaiting: boolean;
  polls: number;
  run: AbortController | null;
  timer: ReturnType<typeof setTimeout> | null;
  error: string;
};

export function usePodcastRefresh(
  account: string,
  rendered: PodcastSubscription,
  episodes: EpisodeUpdates,
) {
  const id = rendered.subscription_id;
  const scope = useRequestScope(JSON.stringify([account, id]));
  const command = usePodcastCommand(account, id);
  const owner = useMemo<Owner>(
    () => ({
      subscription: rendered,
      highestRevision: rendered.content_revision,
      awaiting: false,
      polls: 0,
      run: null,
      timer: null,
      error: "",
    }),
    [account, id],
  );
  const latest = useRef({ owner, rendered, episodes });
  latest.current = { owner, rendered, episodes };
  const [state, setState] = useState<{
    owner: Owner;
    rendered: PodcastSubscription;
    subscription: PodcastSubscription;
    waiting: boolean;
    checking: boolean;
    error: string;
  } | null>(null);
  const current = state?.owner === owner ? state : null;
  const incoming =
    rendered.content_revision >= owner.highestRevision &&
    current?.rendered !== rendered;

  function publish() {
    setState({
      owner,
      rendered: latest.current.rendered,
      subscription: owner.subscription,
      waiting: owner.awaiting || owner.subscription.refresh_pending,
      checking: owner.run !== null,
      error: owner.error,
    });
  }

  function clearTimer() {
    if (owner.timer !== null) clearTimeout(owner.timer);
    owner.timer = null;
  }

  function schedule() {
    clearTimer();
    if (
      !scope.capture().active() ||
      owner.run ||
      (!owner.awaiting && !owner.subscription.refresh_pending)
    )
      return;
    if (owner.polls >= 60) {
      owner.error =
        "This feed check is taking longer than expected. Check again for an update.";
      publish();
      return;
    }
    owner.timer = setTimeout(() => void check(), owner.error ? 15_000 : 5_000);
  }

  async function check(): Promise<void> {
    const request = scope.capture();
    if (!request.active() || owner.run || latest.current.owner !== owner)
      return;
    clearTimer();
    const renderedAtStart = latest.current.rendered;
    const generation = latest.current.episodes.generation;
    const run = new AbortController();
    const signal = AbortSignal.any([request.signal, run.signal]);
    const currentGeneration = () =>
      request.active() &&
      !signal.aborted &&
      latest.current.owner === owner &&
      latest.current.rendered === renderedAtStart &&
      latest.current.episodes.generation === generation;
    const active = () => currentGeneration() && owner.run === run;
    const started = performance.now();
    owner.run = run;
    owner.polls += 1;
    publish();
    try {
      const subscriptions = await podcastApi.subscriptions(account, signal);
      if (!active()) return;
      const subscription = subscriptions.find(
        (item) => item.subscription_id === id,
      );
      if (!subscription)
        throw new Error("The podcast subscription is unavailable.");
      owner.highestRevision = Math.max(
        owner.highestRevision,
        latest.current.episodes.knownRevision,
      );
      if (subscription.content_revision < owner.highestRevision)
        throw new Error("Hito returned an older feed status.");
      owner.subscription = subscription;
      owner.highestRevision = subscription.content_revision;
      owner.awaiting = subscription.refresh_pending;
      owner.error = "";
      if (subscription.refresh_pending) latest.current.episodes.pause();
      else
        void latest.current.episodes.applyRevision(
          subscription.content_revision,
        );
      log.debug("podcasts.refresh_status_checked", {
        account_id: account,
        content_revision: subscription.content_revision,
        enabled: subscription.refresh_pending,
        duration_ms: Math.round(performance.now() - started),
      });
    } catch (error) {
      if (active()) {
        owner.error =
          "Hito could not check this feed. Previously shown episodes remain available.";
        log.warn("podcasts.refresh_status_failed", {
          account_id: account,
          reason: error instanceof HttpError ? "http" : "request",
          status: error instanceof HttpError ? error.status : undefined,
          duration_ms: Math.round(performance.now() - started),
        });
      }
    } finally {
      if (owner.run === run) {
        owner.run = null;
        if (currentGeneration()) {
          publish();
          schedule();
        }
      }
    }
  }

  async function refresh(retry = false): Promise<void> {
    if (!retry && (owner.awaiting || owner.subscription.refresh_pending))
      return;
    const receipt = await (retry ? command.retry() : command.refresh());
    if (!receipt || !scope.capture().active() || latest.current.owner !== owner)
      return;
    owner.awaiting = true;
    owner.polls = 0;
    owner.error = "";
    latest.current.episodes.pause();
    void check();
  }

  useEffect(() => {
    owner.highestRevision = Math.max(
      owner.highestRevision,
      episodes.knownRevision,
    );
    const accepted = rendered.content_revision >= owner.highestRevision;
    if (accepted) {
      owner.subscription = rendered;
      owner.highestRevision = rendered.content_revision;
    }
    if (owner.subscription.refresh_pending || owner.awaiting)
      latest.current.episodes.pause();
    else if (accepted)
      void latest.current.episodes.applyRevision(
        owner.subscription.content_revision,
      );
    publish();
    schedule();
    return () => {
      clearTimer();
      owner.run?.abort();
      owner.run = null;
    };
  }, [owner, scope, rendered, episodes.generation, episodes.knownRevision]);

  return {
    subscription: incoming
      ? rendered
      : (current?.subscription ?? owner.subscription),
    waiting: current?.waiting ?? rendered.refresh_pending,
    checking: current?.checking ?? false,
    statusError: current?.error ?? "",
    pending: command.pending,
    hasAttempt: command.hasAttempt,
    error: command.error,
    refresh: () => refresh(),
    retry: () => refresh(true),
    check: () => {
      owner.polls = 0;
      return check();
    },
  };
}
