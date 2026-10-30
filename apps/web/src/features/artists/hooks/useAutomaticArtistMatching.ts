import { router } from "@inertiajs/react";
import { useEffect } from "react";
import { artistMatchingApi } from "../api/artistMatchingApi";
import { log } from "../../../shared/log";
import { createUuid } from "../../../shared/uuid";

const MAX_BATCHES = 200;
const DEADLINE_MS = 60000;
const PAUSE_MS = 10 * 60 * 1000;

function recentlyChecked(account: string) {
  try {
    const at = Number(localStorage.getItem(`hito:artist-match:${account}`));
    return Number.isFinite(at) && at > 0 && Date.now() - at < PAUSE_MS;
  } catch {
    return false;
  }
}

function remember(account: string): boolean {
  try {
    localStorage.setItem(`hito:artist-match:${account}`, String(Date.now()));
    return true;
  } catch {
    return false;
  }
}

/**
 * Asks the server to match recordings whose own file tags name exactly one performer artist.
 * The pass is silent, cancellable and best effort; the page refreshes only if something matched.
 */
export function useAutomaticArtistMatching(account: string) {
  useEffect(() => {
    if (recentlyChecked(account)) return;
    if (!remember(account)) log.debug("artist_matching.pause_unavailable");
    const controller = new AbortController();
    const timer = setTimeout(() => controller.abort(), DEADLINE_MS);
    const started = performance.now();
    void (async () => {
      let matched = 0;
      let after: string | undefined;
      try {
        for (let batch = 0; batch < MAX_BATCHES; batch += 1) {
          const receipt = await artistMatchingApi.run(
            account,
            { request_id: createUuid(), after },
            controller.signal,
          );
          matched += receipt.matched;
          if (!receipt.next_after) break;
          if (after && receipt.next_after <= after) break;
          after = receipt.next_after;
        }
        log.info("artist_matching.finished", {
          count: matched,
          duration_ms: Math.round(performance.now() - started),
        });
      } catch {
        log.debug("artist_matching.stopped", {
          reason: controller.signal.aborted ? "cancelled" : "failed",
        });
      } finally {
        clearTimeout(timer);
      }
      if (matched > 0 && !controller.signal.aborted)
        router.reload({ only: ["artists", "identity_groups"] });
    })();
    return () => {
      clearTimeout(timer);
      controller.abort();
    };
  }, [account]);
}
