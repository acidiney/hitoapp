import { useEffect, useMemo, useRef, useState } from "react";
import { router } from "@inertiajs/react";
import { log } from "../shared/log";
import { useRequestScope } from "../shared/hooks/useRequestScope";

type ReloadOptions = NonNullable<Parameters<typeof router.reload>[0]>;
type ReloadPage = Parameters<NonNullable<ReloadOptions["onSuccess"]>>[0];
type DetailRun = {
  generation: number;
  target: number;
  outcome: "pending" | "success" | "failed";
  cancel: (() => void) | null;
  abort: () => void;
};
type DetailOwner = {
  generation: number;
  applied: number;
  target: number | null;
  revision: number | null;
  run: DetailRun | null;
};

function record(value: unknown): value is Record<string, unknown> {
  return value !== null && typeof value === "object";
}

function belongs(
  page: ReloadPage,
  account: string,
  playlist: string,
  target: number,
) {
  const user = page.props.user;
  const detail = page.props.detail;
  return (
    page.component === "PlaylistPage" &&
    record(user) &&
    user.id === account &&
    record(detail) &&
    record(detail.playlist) &&
    detail.playlist.id === playlist &&
    detail.playlist.kind === "followed" &&
    typeof detail.follow_content_revision === "number" &&
    Number.isSafeInteger(detail.follow_content_revision) &&
    detail.follow_content_revision >= target
  );
}

export function useFollowDetail(
  account: string,
  playlist: string | null,
  revision: number | null,
) {
  const scope = useRequestScope(JSON.stringify([account, playlist]));
  const owner = useMemo<DetailOwner>(
    () => ({
      generation: 0,
      applied: 0,
      target: revision,
      revision,
      run: null,
    }),
    [account, playlist],
  );
  const rendered = useRef({ owner, revision });
  rendered.current = { owner, revision };
  const [failure, setFailure] = useState<{
    owner: DetailOwner;
    message: string;
  } | null>(null);

  useEffect(() => {
    if (revision !== null && owner.revision !== revision) {
      if (owner.target !== null && revision >= owner.target) {
        owner.applied = Math.max(
          owner.applied,
          owner.run?.generation ?? owner.generation,
        );
        if (owner.run && revision >= owner.run.target)
          owner.run.outcome = "success";
        setFailure({ owner, message: "" });
      }
      owner.target = Math.max(owner.target ?? revision, revision);
    }
    owner.revision = revision;
  }, [owner, revision]);

  useEffect(() => () => owner.run?.abort(), [owner]);

  function markChanged(target: number, completed = false) {
    if (!scope.capture().active()) return;
    if (
      !completed &&
      owner.target !== null &&
      target <= owner.target &&
      owner.generation > owner.applied
    )
      return;
    owner.generation++;
    owner.target = Math.max(owner.target ?? target, target);
  }

  function reload() {
    const request = scope.capture();
    if (
      !playlist ||
      !request.active() ||
      owner.run ||
      owner.target === null ||
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
    const fields = {
      account_id: account,
      playlist_id: playlist,
      content_revision: run.target,
    };
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
          if (!belongs(page, account, playlist, run.target)) {
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
    differsFromRendered: (target: number) =>
      rendered.current.owner === owner &&
      (rendered.current.revision === null ||
        target > rendered.current.revision),
    markChanged,
    reload,
  };
}
