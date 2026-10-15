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
      const jobs = await libraryApi.scan(account, library, signal);
      if (!request.active()) return;
      if (jobs.length === 0) {
        notice("Add a music folder in Settings to begin.");
        return;
      }
      const states = new Map(jobs.map((job) => [job.id, job]));
      for (const job of jobs)
        log.info("scan.queued", { account_id: account, job_id: job.id });
      while (request.active()) {
        const pending = [...states.values()].filter(
          (job) => job.state !== "complete" && job.state !== "failed",
        );
        for (let offset = 0; offset < pending.length; offset += 8) {
          const statuses = await Promise.all(
            pending
              .slice(offset, offset + 8)
              .map((job) => libraryApi.job(account, job.id, signal)),
          );
          if (!request.active()) return;
          for (const status of statuses) {
            states.set(status.id, status);
            log.debug("scan.progress", {
              job_id: status.id,
              count: status.progress,
              mode: status.state,
            });
          }
        }
        const statuses = [...states.values()];
        if (
          statuses.every(
            (job) => job.state === "complete" || job.state === "failed",
          )
        ) {
          const failures = statuses.filter((job) => job.state === "failed");
          const count = statuses.reduce(
            (total, job) => total + job.progress,
            0,
          );
          log.info("scan.finished", {
            account_id: account,
            count,
            mode: failures.length ? "failed" : "complete",
          });
          notice(
            failures.length
              ? `${failures.length} folder scan${failures.length === 1 ? "" : "s"} failed. Check Library settings.`
              : `Scan complete. Read ${count} audio files.`,
          );
          router.reload();
          return;
        }
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
