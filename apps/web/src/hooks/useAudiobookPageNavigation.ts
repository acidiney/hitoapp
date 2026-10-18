import { Link } from "@inertiajs/react";
import {
  useEffect,
  useMemo,
  useRef,
  useState,
  type ComponentProps,
} from "react";
import { log } from "../log";
import { createUuid } from "../uuid";
import { useRequestScope } from "./useRequestScope";

type Run = { cancel: () => void };
type Owner = { run: Run | null; failedHref: string | null; error: string };

export function useAudiobookPageNavigation(account: string, rendered: object) {
  const scope = useRequestScope(account);
  const owner = useMemo<Owner>(
    () => ({ run: null, failedHref: null, error: "" }),
    [account, rendered],
  );
  const latest = useRef(owner);
  latest.current = owner;
  const [state, setState] = useState<{
    owner: Owner;
    pending: boolean;
    failedHref: string | null;
    error: string;
  } | null>(null);
  const current = state?.owner === owner ? state : null;
  function publish() {
    setState({
      owner,
      pending: owner.run !== null,
      failedHref: owner.failedHref,
      error: owner.error,
    });
  }
  useEffect(() => () => owner.run?.cancel(), [owner]);

  function linkProps(href: string): Partial<ComponentProps<typeof Link>> {
    let request: ReturnType<typeof scope.capture> | null = null;
    let run: Run | null = null;
    let token: (() => void) | null = null;
    let timer: ReturnType<typeof setTimeout> | null = null;
    let closed = false;
    let started = 0;
    let requestId: string | undefined;
    let outcome = "pending";
    const active = () =>
      !closed &&
      request?.active() &&
      latest.current === owner &&
      owner.run === run &&
      run !== null;
    const finish = () => {
      if (closed || run === null) return;
      closed = true;
      if (timer !== null) clearTimeout(timer);
      request?.signal.removeEventListener("abort", cancel);
      if (owner.run === run) {
        owner.run = null;
        if (request?.active() && latest.current === owner) publish();
      }
      log.debug("audiobooks.page_finished", {
        account_id: account,
        request_id: requestId,
        reason: outcome,
        duration_ms: Math.round(performance.now() - started),
      });
    };
    const cancel = () => {
      outcome = "retired";
      finish();
      token?.();
    };
    const failed = (reason: string) => {
      if (!active() || outcome !== "pending") return;
      outcome = reason;
      owner.failedHref = href;
      owner.error =
        "Hito could not load this audiobook page. Retry, or return to the first page if your library changed.";
      publish();
      log.warn("audiobooks.page_failed", {
        account_id: account,
        request_id: requestId,
        reason,
        duration_ms: Math.round(performance.now() - started),
      });
    };
    const failAndFinish = (reason: string) => {
      failed(reason);
      finish();
      return false;
    };
    return {
      headers: { "x-hito-account": account, "x-hito-request": "1" },
      onBefore: () => {
        request = scope.capture();
        if (!request?.active() || latest.current !== owner || owner.run)
          return false;
        requestId = createUuid();
        started = performance.now();
        run = { cancel };
        owner.run = run;
        owner.failedHref = null;
        owner.error = "";
        request.signal.addEventListener("abort", cancel, { once: true });
        timer = setTimeout(() => {
          failed("timeout");
          finish();
          token?.();
        }, 10_000);
        publish();
        log.info("audiobooks.page_started", {
          account_id: account,
          request_id: requestId,
        });
        return true;
      },
      onCancelToken: (value) => {
        token = value.cancel;
        if (closed || !request?.active() || latest.current !== owner)
          value.cancel();
      },
      onSuccess: (page) => {
        if (!active()) return;
        const user = page.props.user;
        if (
          page.component !== "AudiobookIndexPage" ||
          !user ||
          typeof user !== "object" ||
          !("id" in user) ||
          user.id !== account ||
          !Array.isArray(page.props.books)
        ) {
          failAndFinish("response");
          return;
        }
        outcome = "loaded";
        owner.failedHref = null;
        owner.error = "";
        finish();
      },
      onNetworkError: () => failAndFinish("network"),
      onHttpException: () => failAndFinish("http"),
      onError: () => {
        failAndFinish("validation");
      },
      onCancel: () => {
        failAndFinish("cancelled");
      },
      onFinish: () => {
        failed("incomplete");
        finish();
      },
    };
  }
  return {
    pending: current?.pending ?? false,
    failedHref: current?.failedHref ?? null,
    error: current?.error ?? "",
    linkProps,
  };
}
