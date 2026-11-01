import { router } from "@inertiajs/react";
import { useCallback, useEffect, useRef, useState } from "react";
import type { Schema } from "../../../shared/schema";
import { log } from "../../../shared/log";
import { useRequestScope } from "../../../shared/hooks/useRequestScope";
import { artistMatchingApi } from "../api/artistMatchingApi";

const ACTIVE_POLL_MS = 3000;
const IDLE_POLL_MS = 15000;

type Status = Schema<"ArtistMatchingStatus">;

function working(status: Status | null) {
  return status !== null && status.enabled && status.state !== "idle";
}

export function useArtistMatchingStatus(account: string) {
  const scope = useRequestScope(account);
  const [status, setStatus] = useState<Status | null>(null);
  const readNow = useRef<() => void>(() => {});

  useEffect(() => {
    const request = scope.capture();
    let timer: ReturnType<typeof setTimeout> | undefined;
    let previous: Status | null = null;
    let reading = false;
    setStatus(null);

    const schedule = () => {
      clearTimeout(timer);
      if (!request.active() || document.visibilityState !== "visible") return;
      if (previous && !previous.enabled) return;
      timer = setTimeout(
        read,
        working(previous) ? ACTIVE_POLL_MS : IDLE_POLL_MS,
      );
    };

    async function read() {
      clearTimeout(timer);
      if (reading) return;
      reading = true;
      try {
        const next = await artistMatchingApi.status(account, request.signal);
        if (!request.active()) return;
        if (working(previous) && !working(next)) {
          log.debug("artist_matching.walk_finished", {
            count: next.last_matched,
          });
          router.reload({ only: ["artists", "identity_groups"] });
        }
        previous = next;
        setStatus(next);
      } catch {
        log.debug("artist_matching.status_unavailable");
      } finally {
        reading = false;
        schedule();
      }
    }

    const visibility = () => {
      if (document.visibilityState === "visible") void read();
      else clearTimeout(timer);
    };
    readNow.current = () => void read();
    document.addEventListener("visibilitychange", visibility);
    void read();
    return () => {
      clearTimeout(timer);
      document.removeEventListener("visibilitychange", visibility);
    };
  }, [account, scope]);

  const refresh = useCallback(() => readNow.current(), []);
  return { status, refresh };
}
