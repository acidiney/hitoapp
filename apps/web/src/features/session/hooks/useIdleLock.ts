import type { Schema } from "../../../shared/schema";
import { useEffect, useRef } from "react";
import { router } from "@inertiajs/react";

import { lockApi } from "../api/lockApi";

const inputEvents = ["pointerdown", "keydown", "wheel", "touchstart"] as const;
const heartbeatMs = 30_000;

export function useIdleLock(
  account: string,
  lock: Schema<"LockStatus">,
  onLocked: () => void,
) {
  const lockedRef = useRef(onLocked);
  lockedRef.current = onLocked;
  const { pin_enabled: enabled, idle_minutes: idleMinutes } = lock;
  useEffect(() => {
    if (!enabled || idleMinutes === 0) return;
    let timer: ReturnType<typeof setTimeout> | undefined;
    let lastBeat = 0;
    let stopped = false;
    const controller = new AbortController();
    function lockedNow() {
      if (stopped) return;
      stopped = true;
      lockedRef.current();
      router.clearHistory();
      window.location.reload();
    }
    async function report(request: Promise<Schema<"LockStatus">>) {
      try {
        if ((await request).locked) lockedNow();
      } catch {}
    }
    function arm() {
      clearTimeout(timer);
      timer = setTimeout(
        () => {
          void report(lockApi.state(account, controller.signal)).then(() => {
            if (!stopped) arm();
          });
        },
        idleMinutes * 60_000 + 1_000,
      );
    }
    function input() {
      if (stopped) return;
      arm();
      const now = Date.now();
      if (now - lastBeat >= heartbeatMs) {
        lastBeat = now;
        void report(lockApi.activity(account, controller.signal));
      }
    }
    for (const name of inputEvents)
      document.addEventListener(name, input, { passive: true });
    arm();
    return () => {
      stopped = true;
      controller.abort();
      clearTimeout(timer);
      for (const name of inputEvents) document.removeEventListener(name, input);
    };
  }, [account, enabled, idleMinutes]);
}
