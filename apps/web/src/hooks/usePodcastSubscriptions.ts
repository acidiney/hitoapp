import { useEffect, useMemo, useState } from "react";
import { HttpError } from "../http";
import { log } from "../log";
import { podcastApi } from "../podcastApi";
import type { PodcastSubscription, SubscribePodcast } from "../types";
import { createUuid } from "../uuid";
import { useRequestScope } from "./useRequestScope";

type Run = { controller: AbortController };
type Owner = {
  subscriptions: PodcastSubscription[];
  attempt: Readonly<SubscribePodcast> | null;
  read: Run | null;
  write: Run | null;
  timer: ReturnType<typeof setTimeout> | null;
  awaitingSubscription: string | null;
  polls: number;
  error: string;
  statusError: string;
};
type State = {
  owner: Owner;
  subscriptions: PodcastSubscription[];
  pending: boolean;
  hasAttempt: boolean;
  loading: boolean;
  error: string;
  statusError: string;
};

export function usePodcastSubscriptions(
  account: string,
  rendered: PodcastSubscription[],
) {
  const scope = useRequestScope(account);
  const owner = useMemo<Owner>(
    () => ({
      subscriptions: rendered,
      attempt: null,
      read: null,
      write: null,
      timer: null,
      awaitingSubscription: null,
      polls: 0,
      error: "",
      statusError: "",
    }),
    [account],
  );
  const [state, setState] = useState<State | null>(null);
  const current = state?.owner === owner ? state : null;

  function publish() {
    setState({
      owner,
      subscriptions: owner.subscriptions,
      pending: owner.write !== null,
      hasAttempt: owner.attempt !== null,
      loading: owner.read !== null,
      error: owner.error,
      statusError: owner.statusError,
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
      owner.read ||
      owner.write ||
      (!owner.awaitingSubscription &&
        !owner.subscriptions.some((item) => item.refresh_pending))
    )
      return;
    if (owner.polls >= 60) {
      owner.statusError =
        "Feed checks are taking longer than expected. Check again for an update.";
      publish();
      return;
    }
    owner.timer = setTimeout(
      () => void load(),
      owner.statusError ? 15_000 : 5_000,
    );
  }

  async function load(): Promise<void> {
    const request = scope.capture();
    if (!request.active() || owner.read || owner.write) return;
    clearTimer();
    const run = { controller: new AbortController() };
    const signal = AbortSignal.any([request.signal, run.controller.signal]);
    const active = () =>
      request.active() && !signal.aborted && owner.read === run;
    const started = performance.now();
    owner.read = run;
    owner.polls += 1;
    publish();
    try {
      const subscriptions = await podcastApi.subscriptions(account, signal);
      if (!active()) return;
      owner.subscriptions = subscriptions;
      if (
        subscriptions.some(
          (item) => item.subscription_id === owner.awaitingSubscription,
        )
      )
        owner.awaitingSubscription = null;
      owner.statusError = "";
      log.debug("podcasts.status_checked", {
        account_id: account,
        count: subscriptions.length,
        duration_ms: Math.round(performance.now() - started),
      });
    } catch (error) {
      if (active()) {
        owner.statusError =
          "Hito could not check podcast updates. Your last checked subscriptions are still shown.";
        log.warn("podcasts.status_failed", {
          account_id: account,
          reason: error instanceof HttpError ? "http" : "request",
          status: error instanceof HttpError ? error.status : undefined,
          duration_ms: Math.round(performance.now() - started),
        });
      }
    } finally {
      if (owner.read === run) {
        owner.read = null;
        if (request.active() && !signal.aborted) {
          publish();
          schedule();
        }
      }
    }
  }

  async function perform(body: Readonly<SubscribePodcast>): Promise<boolean> {
    const request = scope.capture();
    if (!request.active() || owner.write) return false;
    clearTimer();
    owner.read?.controller.abort();
    owner.read = null;
    const run = { controller: new AbortController() };
    const signal = AbortSignal.any([request.signal, run.controller.signal]);
    const active = () =>
      request.active() && !signal.aborted && owner.write === run;
    const started = performance.now();
    const fields = { account_id: account, operation_id: body.request_id };
    owner.write = run;
    owner.attempt = body;
    owner.error = "";
    publish();
    log.info("podcasts.subscribe_started", fields);
    let accepted = false;
    try {
      const receipt = await podcastApi.subscribe(account, body, signal);
      if (!active()) return false;
      if (
        receipt.request_id !== body.request_id ||
        typeof receipt.subscription_id !== "string" ||
        !receipt.subscription_id ||
        typeof receipt.job_id !== "string" ||
        !receipt.job_id
      )
        throw new Error("The subscription receipt did not match this request.");
      owner.attempt = null;
      owner.awaitingSubscription = receipt.subscription_id;
      owner.polls = 0;
      accepted = true;
      log.info("podcasts.subscribe_accepted", {
        ...fields,
        job_id: receipt.job_id,
        duration_ms: Math.round(performance.now() - started),
      });
      return true;
    } catch (error) {
      if (active()) {
        const rejected =
          error instanceof HttpError &&
          [400, 413, 415, 422].includes(error.status);
        if (rejected) owner.attempt = null;
        owner.error = rejected
          ? "Hito rejected the subscription request. Check the feed address and try again."
          : "Hito could not confirm the subscription. Retry the same request to recover its result.";
        log.warn(
          rejected
            ? "podcasts.subscribe_rejected"
            : "podcasts.subscribe_failed",
          {
            ...fields,
            reason: error instanceof HttpError ? "http" : "request",
            status: error instanceof HttpError ? error.status : undefined,
            duration_ms: Math.round(performance.now() - started),
          },
        );
      }
      return false;
    } finally {
      if (owner.write === run) {
        owner.write = null;
        if (request.active() && !signal.aborted) {
          publish();
          if (accepted) void load();
          else schedule();
        }
      }
      if (!request.active() || signal.aborted)
        log.debug("podcasts.subscribe_cancelled", fields);
    }
  }

  useEffect(() => {
    owner.read?.controller.abort();
    owner.read = null;
    owner.subscriptions = rendered;
    owner.polls = 0;
    publish();
    schedule();
    return () => {
      clearTimer();
      owner.read?.controller.abort();
      owner.read = null;
    };
  }, [owner, scope, rendered]);

  useEffect(
    () => () => {
      owner.write?.controller.abort();
      owner.write = null;
    },
    [owner, scope],
  );

  return {
    subscriptions: current?.subscriptions ?? rendered,
    pending: current?.pending ?? false,
    hasAttempt: current?.hasAttempt ?? false,
    loading: current?.loading ?? false,
    error: current?.error ?? "",
    statusError: current?.statusError ?? "",
    subscribe: (address: string) => {
      if (owner.attempt || owner.write || !scope.capture().active())
        return Promise.resolve(false);
      const feed = address.trim();
      if (
        !feed ||
        new TextEncoder().encode(feed).byteLength > 2048 ||
        /[\u0000-\u001f\u007f-\u009f]/u.test(feed)
      ) {
        owner.error = "Use a feed address of up to 2,048 bytes.";
        publish();
        return Promise.resolve(false);
      }
      return perform(
        Object.freeze({ request_id: createUuid(), feed_url: feed }),
      );
    },
    retry: () =>
      owner.attempt ? perform(owner.attempt) : Promise.resolve(false),
    check: () => {
      owner.polls = 0;
      return load();
    },
  };
}
