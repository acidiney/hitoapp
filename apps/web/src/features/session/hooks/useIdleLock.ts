import { useEffect, useRef } from "react";
import type { LockStatus } from "../../../shared/types";
import { lockApi } from "../api/lockApi";

const inputEvents = ["pointerdown", "keydown", "wheel", "touchstart"] as const;
const heartbeatMs = 30_000;

export function useIdleLock(
  account: string,
  lock: LockStatus,
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
    async function poll() {
      try {
        const status = await lockApi.state(account, controller.signal);
        if (status.locked && !stopped) {
          stopped = true;
          lockedRef.current();
          window.location.reload();
        }
      } catch {}
    }
    function arm() {
      clearTimeout(timer);
      timer = setTimeout(
        () => {
          void poll().then(() => {
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
        void poll();
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
