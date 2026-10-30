import { router } from "@inertiajs/react";
import { useCallback, useEffect, useRef, useState } from "react";
import type { Schema } from "../../../shared/schema";
import { log } from "../../../shared/log";
import { createUuid } from "../../../shared/uuid";
import { useRequestScope } from "../../../shared/hooks/useRequestScope";
import { metadataPreferencesApi } from "../../metadata/api/metadataPreferencesApi";
import { artistMatchingApi } from "../api/artistMatchingApi";

const MAX_BATCHES = 200;
const DEADLINE_MS = 120000;

export type ProviderMatchCounts = {
  matched: number;
  uncertain: number;
  unavailable: number;
};

export type ProviderMatchState =
  | { status: "checking" | "disabled" | "ready" | "running" | "failed" }
  | { status: "finished"; counts: ProviderMatchCounts; complete: boolean };

function builtinEnabled(settings: Schema<"MetadataSettings">) {
  return (
    settings.enabled &&
    settings.providers.some(
      (provider) => provider.kind === "builtin_musicbrainz",
    )
  );
}

const NAME_PHASE = "name:";

/** A cursor moves forward within a phase; entering the name phase is always progress. */
function advances(previous: string | undefined, next: string) {
  if (previous === undefined) return true;
  if (previous.startsWith(NAME_PHASE) !== next.startsWith(NAME_PHASE))
    return next.startsWith(NAME_PHASE);
  return next > previous;
}

/**
 * The provider pass sends ISRC codes, provider artist links and credit names to MusicBrainz, so it
 * starts only when the account asks for it and only while its metadata settings allow the
 * built-in provider. It walks the server's cursor through the evidence phase and then the name
 * phase, and leaving the page cancels whatever is in flight.
 */
export function useProviderArtistMatching(account: string) {
  const scope = useRequestScope(account);
  const [state, setState] = useState<ProviderMatchState>({
    status: "checking",
  });
  const running = useRef<AbortController | null>(null);

  useEffect(() => {
    const request = scope.capture();
    setState({ status: "checking" });
    void metadataPreferencesApi
      .settings(account, request.signal)
      .then((settings) => {
        if (request.active())
          setState({ status: builtinEnabled(settings) ? "ready" : "disabled" });
      })
      .catch(() => {
        if (request.active()) setState({ status: "failed" });
      });
    return () => running.current?.abort();
  }, [account, scope]);

  const run = useCallback(async () => {
    if (running.current) return;
    const request = scope.capture();
    const controller = new AbortController();
    running.current = controller;
    const signal = AbortSignal.any([request.signal, controller.signal]);
    const timer = setTimeout(() => controller.abort(), DEADLINE_MS);
    const started = performance.now();
    const counts: ProviderMatchCounts = {
      matched: 0,
      uncertain: 0,
      unavailable: 0,
    };
    let complete = false;
    setState({ status: "running" });
    try {
      let after: string | undefined;
      for (let batch = 0; batch < MAX_BATCHES; batch += 1) {
        const receipt = await artistMatchingApi.runProviderEvidence(
          account,
          { request_id: createUuid(), after },
          signal,
        );
        counts.matched += receipt.matched;
        counts.uncertain += receipt.uncertain;
        counts.unavailable += receipt.unavailable;
        if (!receipt.next_after) {
          complete = true;
          break;
        }
        if (receipt.unavailable > 0) break;
        if (!advances(after, receipt.next_after)) break;
        after = receipt.next_after;
      }
      if (!request.active()) return;
      log.info("artist_matching.provider_finished", {
        count: counts.matched,
        duration_ms: Math.round(performance.now() - started),
      });
      setState({ status: "finished", counts, complete });
      if (counts.matched > 0)
        router.reload({ only: ["artists", "identity_groups"] });
    } catch {
      log.debug("artist_matching.provider_stopped", {
        reason: signal.aborted ? "cancelled" : "failed",
      });
      if (request.active())
        setState(
          counts.matched + counts.uncertain + counts.unavailable > 0
            ? { status: "finished", counts, complete: false }
            : { status: "failed" },
        );
    } finally {
      clearTimeout(timer);
      if (running.current === controller) running.current = null;
    }
  }, [account, scope]);

  return { state, run };
}
