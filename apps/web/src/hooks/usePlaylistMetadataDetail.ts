import { useEffect, useMemo, useState } from "react";
import { router } from "@inertiajs/react";
import { log } from "../shared/log";
import { useRequestScope } from "../shared/hooks/useRequestScope";

type DetailOwner = { cancel: (() => void) | null; error: string };
type DetailState = { owner: DetailOwner; pending: boolean; error: string };
type ReloadOptions = NonNullable<Parameters<typeof router.reload>[0]>;
type ReloadPage = Parameters<NonNullable<ReloadOptions["onSuccess"]>>[0];

function record(value: unknown): value is Record<string, unknown> {
  return value !== null && typeof value === "object";
}

function belongs(page: ReloadPage, account: string, playlist: string) {
  const props = page.props;
  return (
    page.component === "PlaylistPage" &&
    record(props) &&
    record(props.user) &&
    props.user.id === account &&
    record(props.detail) &&
    record(props.detail.playlist) &&
    props.detail.playlist.id === playlist &&
    props.detail.playlist.kind === "local"
  );
}

export function usePlaylistMetadataDetail(
  account: string,
  playlist: string | null,
  key: string,
) {
  const scope = useRequestScope(key);
  const owner = useMemo<DetailOwner>(
    () => ({ cancel: null, error: "" }),
    [key],
  );
  const [state, setState] = useState<DetailState | null>(null);
  const current = state?.owner === owner ? state : null;
  useEffect(() => () => owner.cancel?.(), [owner]);

  function publish() {
    setState({ owner, pending: owner.cancel !== null, error: owner.error });
  }

  function reload() {
    const request = scope.capture();
    if (!playlist || !request.active() || owner.cancel) return;
    let token: (() => void) | null = null;
    let closed = false;
    let outcome: "pending" | "success" | "failed" = "pending";
    const started = performance.now();
    const fields = { account_id: account, playlist_id: playlist };
    const release = () => {
      closed = true;
      request.signal.removeEventListener("abort", cancel);
      if (owner.cancel === cancel) {
        owner.cancel = null;
        if (request.active()) publish();
      }
    };
    const cancel = () => {
      if (closed) return;
      release();
      token?.();
      log.debug("playlist_metadata.details_cancelled", fields);
    };
    const active = () => !closed && request.active() && owner.cancel === cancel;
    const failed = (reason: string) => {
      if (!active() || outcome !== "pending") return;
      outcome = "failed";
      owner.error =
        "Album information was saved, but this playlist's details could not be loaded. Reload its details to refresh this view.";
      publish();
      log.warn("playlist_metadata.details_failed", {
        ...fields,
        reason,
        duration_ms: Math.round(performance.now() - started),
      });
    };
    owner.cancel = cancel;
    request.signal.addEventListener("abort", cancel, { once: true });
    publish();
    log.debug("playlist_metadata.details_started", fields);
    try {
      router.reload({
        only: ["detail", "playlists"],
        onBefore: active,
        onCancelToken: (value) => {
          token = value.cancel;
          if (closed || !request.active()) value.cancel();
        },
        onSuccess: (page) => {
          if (!active() || outcome !== "pending") return;
          if (!belongs(page, account, playlist)) {
            failed("response");
            return;
          }
          outcome = "success";
          owner.error = "";
          publish();
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
        onFinish: () => {
          if (closed) return;
          failed("incomplete");
          release();
          log.debug("playlist_metadata.details_finished", {
            ...fields,
            mode: outcome,
            duration_ms: Math.round(performance.now() - started),
          });
        },
      });
    } catch {
      failed("request");
      cancel();
    }
  }

  return {
    pending: current?.pending ?? false,
    error: current?.error ?? "",
    reload,
    cancel: () => {
      owner.cancel?.();
      owner.error = "";
      if (scope.capture().active()) publish();
    },
  };
}
