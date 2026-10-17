import { useEffect, useMemo, useState } from "react";
import { HttpError } from "../http";
import { log } from "../log";
import { podcastApi } from "../podcastApi";
import type { PodcastRefreshReceipt, RefreshPodcast } from "../types";
import { createUuid } from "../uuid";
import { useRequestScope } from "./useRequestScope";

type Owner = {
  attempt: Readonly<RefreshPodcast> | null;
  run: AbortController | null;
};

export function usePodcastCommand(account: string, subscription: string) {
  const scope = useRequestScope(JSON.stringify([account, subscription]));
  const owner = useMemo<Owner>(
    () => ({ attempt: null, run: null }),
    [account, subscription],
  );
  const [state, setState] = useState<{
    owner: Owner;
    pending: boolean;
    hasAttempt: boolean;
    error: string;
  } | null>(null);
  const current = state?.owner === owner ? state : null;

  async function perform(
    body: Readonly<RefreshPodcast>,
  ): Promise<PodcastRefreshReceipt | null> {
    const request = scope.capture();
    if (!request.active() || owner.run) return null;
    const run = new AbortController();
    const signal = AbortSignal.any([request.signal, run.signal]);
    const active = () =>
      request.active() && !signal.aborted && owner.run === run;
    const started = performance.now();
    const fields = { account_id: account, operation_id: body.request_id };
    owner.run = run;
    owner.attempt = body;
    setState({ owner, pending: true, hasAttempt: true, error: "" });
    log.info("podcasts.refresh_started", fields);
    let message = "";
    try {
      const receipt = await podcastApi.refresh(
        account,
        subscription,
        body,
        signal,
      );
      if (!active()) return null;
      if (
        receipt.request_id !== body.request_id ||
        receipt.subscription_id !== subscription ||
        typeof receipt.job_id !== "string" ||
        !receipt.job_id
      )
        throw new Error("The refresh receipt did not match this request.");
      owner.attempt = null;
      log.info("podcasts.refresh_accepted", {
        ...fields,
        job_id: receipt.job_id,
        duration_ms: Math.round(performance.now() - started),
      });
      return receipt;
    } catch (error) {
      if (active()) {
        const rejected =
          error instanceof HttpError &&
          [400, 413, 415, 422].includes(error.status);
        if (rejected) owner.attempt = null;
        message = rejected
          ? "Hito rejected the refresh request. Try again."
          : "Hito could not confirm the refresh. Retry the same request to recover its result.";
        log.warn(
          rejected ? "podcasts.refresh_rejected" : "podcasts.refresh_failed",
          {
            ...fields,
            reason: error instanceof HttpError ? "http" : "request",
            status: error instanceof HttpError ? error.status : undefined,
            duration_ms: Math.round(performance.now() - started),
          },
        );
      }
      return null;
    } finally {
      if (owner.run === run) {
        owner.run = null;
        if (request.active() && !signal.aborted)
          setState({
            owner,
            pending: false,
            hasAttempt: owner.attempt !== null,
            error: message,
          });
      }
      if (!request.active() || signal.aborted)
        log.debug("podcasts.refresh_cancelled", fields);
    }
  }

  useEffect(() => () => owner.run?.abort(), [owner]);

  return {
    pending: current?.pending ?? false,
    hasAttempt: current?.hasAttempt ?? false,
    error: current?.error ?? "",
    refresh: () =>
      owner.attempt
        ? Promise.resolve(null)
        : perform(Object.freeze({ request_id: createUuid() })),
    retry: () =>
      owner.attempt ? perform(owner.attempt) : Promise.resolve(null),
  };
}
