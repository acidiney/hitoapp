import type { Schema } from "../../../shared/schema";
import { useState } from "react";
import { log } from "../../../shared/log";

import { useRequestScope } from "../../../shared/hooks/useRequestScope";
import { albumEditionChoiceApi } from "../api/albumEditionChoiceApi";

const REQUEST_DEADLINE_MS = 20_000;

export type TaggedLookup = Schema<"SavedAlbumEdition"> | "none" | "stopped";

export function useTaggedAlbumEdition(account: string, release: string) {
  const scope = useRequestScope(`${account}/${release}/tagged`);
  const [pending, setPending] = useState(false);

  async function find(): Promise<TaggedLookup> {
    const request = scope.capture();
    const controller = new AbortController();
    const cancel = () => controller.abort();
    request.signal.addEventListener("abort", cancel, { once: true });
    const timer = setTimeout(() => controller.abort(), REQUEST_DEADLINE_MS);
    const started = performance.now();
    let outcome = "none";
    setPending(true);
    try {
      const result = await albumEditionChoiceApi.tagged(
        account,
        release,
        controller.signal,
      );
      if (!request.active()) return (outcome = "stopped");
      if (result.status === "found") {
        outcome = "found";
        return result.edition;
      }
      return "none";
    } catch {
      if (!request.active()) return (outcome = "stopped");
      outcome = "failed";
      return "none";
    } finally {
      clearTimeout(timer);
      request.signal.removeEventListener("abort", cancel);
      if (request.active()) setPending(false);
      const record = outcome === "failed" ? log.warn : log.debug;
      record("album_edition.tagged.finished", {
        reason: outcome,
        duration_ms: Math.round(performance.now() - started),
      });
    }
  }

  return { find, pending };
}
