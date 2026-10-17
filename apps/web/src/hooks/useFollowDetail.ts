import { useEffect, useMemo, useRef, useState } from "react";
import { router } from "@inertiajs/react";
import { log } from "../log";
import { useRequestScope } from "./useRequestScope";

type ReloadOptions = NonNullable<Parameters<typeof router.reload>[0]>;
type ReloadPage = Parameters<NonNullable<ReloadOptions["onSuccess"]>>[0];
type DetailRun = {
  generation: number;
  target: string | null;
  outcome: "pending" | "success" | "failed";
  cancel: (() => void) | null;
  abort: () => void;
};
type DetailOwner = {
  generation: number;
  applied: number;
  target: string | null;
  snapshot: string | null;
  run: DetailRun | null;
};

function record(value: unknown): value is Record<string, unknown> {
  return value !== null && typeof value === "object";
}

function belongs(page: ReloadPage, account: string, playlist: string) {
  const user = page.props.user;
  const detail = page.props.detail;
  return (
    page.component === "PlaylistPage" &&
    record(user) &&
    user.id === account &&
    record(detail) &&
    record(detail.playlist) &&
    detail.playlist.id === playlist &&
    detail.playlist.kind === "followed"
  );
}

export function useFollowDetail(
  account: string,
  playlist: string | null,
  snapshot: string | null,
) {
  const scope = useRequestScope(JSON.stringify([account, playlist]));
  const owner = useMemo<DetailOwner>(
    () => ({ generation: 0, applied: 0, target: null, snapshot, run: null }),
    [account, playlist],
  );
  const rendered = useRef({ owner, snapshot });
  rendered.current = { owner, snapshot };
  const [failure, setFailure] = useState<{
    owner: DetailOwner;
    message: string;
  } | null>(null);

  useEffect(() => {
    if (owner.snapshot !== snapshot && owner.target === snapshot) {
      owner.applied = owner.run?.generation ?? owner.generation;
      if (owner.run?.target === snapshot) owner.run.outcome = "success";
      setFailure({ owner, message: "" });
    }
    owner.snapshot = snapshot;
  }, [owner, snapshot]);

  useEffect(() => () => owner.run?.abort(), [owner]);

  function markChanged(target: string) {
    if (!scope.capture().active()) return;
    owner.generation++;
    owner.target = target;
  }

  function reload() {
    const request = scope.capture();
    if (
      !playlist ||
      !request.active() ||
      owner.run ||
      owner.generation <= owner.applied
    )
      return;
    let run: DetailRun;
    const release = () => request.signal.removeEventListener("abort", abort);
    const abort = () => {
      release();
      if (owner.run === run) owner.run = null;
      run.cancel?.();
    };
    run = {
      generation: owner.generation,
      target: owner.target,
      outcome: "pending",
      cancel: null,
      abort,
    };
    owner.run = run;
    request.signal.addEventListener("abort", abort, { once: true });
    const active = () => request.active() && owner.run === run;
    const started = performance.now();
    const fields = { account_id: account, playlist_id: playlist };
    function failed(reason: string) {
      if (!active() || run.outcome !== "pending") return;
      run.outcome = "failed";
      setFailure({
        owner,
        message:
          "The playlist update finished, but its details could not be loaded. Retry to refresh this view.",
      });
      log.warn("playlist_follow.details_failed", {
        ...fields,
        reason,
        duration_ms: Math.round(performance.now() - started),
      });
    }
    function finished() {
      release();
      if (!active()) return;
      failed("incomplete");
      owner.run = null;
    }
    log.info("playlist_follow.details_requested", fields);
    try {
      router.reload({
        only: ["detail", "playlists"],
        onBefore: () => active(),
        onCancelToken: (token) => {
          run.cancel = token.cancel;
          if (!active()) token.cancel();
        },
        onSuccess: (page) => {
          if (!active()) return;
          if (!belongs(page, account, playlist)) {
            failed("response");
            return;
          }
          run.outcome = "success";
          owner.applied = Math.max(owner.applied, run.generation);
          setFailure({ owner, message: "" });
          log.info("playlist_follow.details_applied", {
            ...fields,
            duration_ms: Math.round(performance.now() - started),
          });
        },
        onNetworkError: () => {
          failed("network");
          return false;
        },
        onHttpException: () => {
          failed("http");
          return false;
        },
        onError: () => failed("validation"),
        onCancel: () => failed("cancelled"),
        onFinish: finished,
      });
    } catch {
      failed("request");
      finished();
      run.cancel?.();
    }
  }

  return {
    detailError: failure?.owner === owner ? failure.message : "",
    differsFromRendered: (target: string) =>
      rendered.current.owner === owner &&
      rendered.current.snapshot !== null &&
      rendered.current.snapshot !== target,
    markChanged,
    reload,
  };
}
