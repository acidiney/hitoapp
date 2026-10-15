import { useCallback, useRef, useState } from "react";
import { router } from "@inertiajs/react";
import { libraryApi } from "../api";
import { errorMessage } from "../format";
import { useNotice } from "../components/Notices";
import { useRequestScope } from "./useRequestScope";
import { log } from "../log";

function waitForPoll(signal: AbortSignal) {
  return new Promise<void>((resolve, reject) => {
    const timer = setTimeout(() => {
      signal.removeEventListener("abort", cancel);
      resolve();
    }, 1000);
    function cancel() {
      clearTimeout(timer);
      reject(signal.reason);
    }
    signal.addEventListener("abort", cancel, { once: true });
    if (signal.aborted) cancel();
  });
}

export function useScanJob(account: string, library: string | undefined) {
  const notice = useNotice();
  const scope = useRequestScope(`${account}\0${library ?? ""}`);
  const [scanningScope, setScanningScope] = useState<typeof scope | null>(null);
  const current = useRef<typeof scope | null>(null);

  const scan = useCallback(async () => {
    if (!library || current.current === scope) return;
    const request = scope.capture();
    const { signal } = request;
    current.current = scope;
    setScanningScope(scope);
    log.info("scan.started", { account_id: account });
    try {
      const job = await libraryApi.scan(account, library, signal);
      if (!request.active()) return;
      log.info("scan.queued", { account_id: account, job_id: job.id });
      while (request.active()) {
        const status = await libraryApi.job(account, job.id, signal);
        if (!request.active()) return;
        if (status.state === "complete" || status.state === "failed") {
          log.info("scan.finished", {
            account_id: account,
            job_id: job.id,
            mode: status.state,
            count: status.progress,
          });
          notice(
            status.error ??
              `Scan complete. Read ${status.progress} audio files.`,
          );
          router.reload();
          return;
        }
        log.debug("scan.progress", {
          job_id: job.id,
          count: status.progress,
          mode: status.state,
        });
        await waitForPoll(signal);
      }
    } catch (error) {
      if (request.active()) {
        log.warn("scan.failed", { account_id: account });
        notice(errorMessage(error));
      }
    } finally {
      if (current.current === scope) current.current = null;
      if (request.active()) setScanningScope(null);
      else log.debug("scan.cancelled", { account_id: account });
    }
  }, [account, library, notice, scope]);
  return { scan, scanning: scanningScope === scope };
}
