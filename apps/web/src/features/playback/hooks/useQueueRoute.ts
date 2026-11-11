import { useEffect, useRef } from "react";
import { router } from "@inertiajs/react";
import { setQueuePanelOpen } from "../queuePanelState";
import { setQueueSheetOpen } from "../queueSheetState";

export const QUEUE_PATH = "/queue";

export function useQueueRoute(
  pathname: string,
  search: string,
  phone: boolean,
) {
  const previous = useRef<string | null>(null);
  useEffect(() => {
    if (pathname !== QUEUE_PATH) {
      previous.current = `${pathname}${search}`;
      return;
    }
    if (phone) setQueueSheetOpen(true);
    else setQueuePanelOpen(true);
    router.visit(previous.current ?? "/home", { replace: true });
  }, [pathname, search, phone]);
}
