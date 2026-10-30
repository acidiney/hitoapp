import type { Schema } from "../../../shared/schema";
import { useEffect, useMemo, useState } from "react";
import { HttpError } from "../../../shared/http";
import { log } from "../../../shared/log";
import { podcastApi } from "../api/podcastApi";

import { createUuid } from "../../../shared/uuid";
import { useRequestScope } from "../../../shared/hooks/useRequestScope";

type Attempt =
  | { kind: "refresh"; body: Readonly<Schema<"RefreshPodcast">> }
  | { kind: "unsubscribe"; body: Readonly<Schema<"UnsubscribePodcast">> };
type Accepted =
  | { kind: "refresh"; receipt: Schema<"PodcastRefreshReceipt"> }
  | { kind: "unsubscribe"; receipt: Schema<"PodcastUnsubscribeReceipt"> };
type Owner = {
  attempt: Attempt | null;
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
    attemptKind: Attempt["kind"] | null;
    error: string;
  } | null>(null);
  const current = state?.owner === owner ? state : null;
  const mayReloadDetail = useMemo(
    () => () =>
      scope.capture().active() && owner.attempt?.kind !== "unsubscribe",
    [owner, scope],
  );

  async function perform(attempt: Attempt): Promise<Accepted | null> {
    const request = scope.capture();
    if (!request.active() || owner.run) return null;
    const run = new AbortController();
    const signal = AbortSignal.any([request.signal, run.signal]);
    const active = () =>
      request.active() && !signal.aborted && owner.run === run;
    const started = performance.now();
    const fields = {
      account_id: account,
      operation_id: attempt.body.request_id,
      action: attempt.kind,
    };
    owner.run = run;
    owner.attempt = attempt;
    setState({ owner, pending: true, attemptKind: attempt.kind, error: "" });
    log.info(`podcasts.${attempt.kind}_started`, fields);
    let message = "";
    try {
      const result: Accepted =
        attempt.kind === "refresh"
          ? {
              kind: "refresh",
              receipt: await podcastApi.refresh(
                account,
                subscription,
                attempt.body,
                signal,
              ),
            }
          : {
              kind: "unsubscribe",
              receipt: await podcastApi.unsubscribe(
                account,
                subscription,
                attempt.body,
                signal,
              ),
            };
      if (!active()) return null;
      if (
        result.receipt.request_id !== attempt.body.request_id ||
        result.receipt.subscription_id !== subscription ||
        (result.kind === "refresh" &&
          (typeof result.receipt.job_id !== "string" || !result.receipt.job_id))
      )
        throw new Error("The podcast receipt did not match this request.");
      const keepsFence = result.kind === "unsubscribe";
      if (!keepsFence) owner.attempt = null;
      log.info(`podcasts.${attempt.kind}_accepted`, {
        ...fields,
        job_id: result.kind === "refresh" ? result.receipt.job_id : undefined,
        duration_ms: Math.round(performance.now() - started),
      });
      return result;
    } catch (error) {
      if (active()) {
        const rejected =
          error instanceof HttpError &&
          [400, 413, 415, 422].includes(error.status);
        if (rejected) owner.attempt = null;
        message = rejected
          ? `Hito rejected the ${attempt.kind} request. Try again.`
          : `Hito could not confirm the ${attempt.kind} request. Retry the same request to recover its result.`;
        log.warn(
          `podcasts.${attempt.kind}_${rejected ? "rejected" : "failed"}`,
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
            attemptKind: owner.attempt?.kind ?? null,
            error: message,
          });
      }
      if (!request.active() || signal.aborted)
        log.debug(`podcasts.${attempt.kind}_cancelled`, fields);
    }
  }

  useEffect(() => () => owner.run?.abort(), [owner]);

  function start(kind: Attempt["kind"]): Promise<Accepted | null> {
    if (owner.attempt) return Promise.resolve(null);
    const body = Object.freeze({ request_id: createUuid() });
    return perform(
      kind === "refresh"
        ? { kind: "refresh", body }
        : { kind: "unsubscribe", body },
    );
  }

  return {
    pending: current?.pending ?? false,
    hasAttempt: current?.attemptKind != null,
    attemptKind: current?.attemptKind ?? null,
    error: current?.error ?? "",
    mayReloadDetail,
    refresh: () => start("refresh"),
    unsubscribe: () => start("unsubscribe"),
    retry: () =>
      owner.attempt ? perform(owner.attempt) : Promise.resolve(null),
  };
}
