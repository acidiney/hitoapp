import { useMemo, useState } from "react";
import { router } from "@inertiajs/react";
import { prepareChange, submitChange } from "../../../shared/sync";
import { audiobookApi } from "../api/audiobookApi";
import { errorMessage } from "../../../shared/format";
import { HttpError } from "../../../shared/http";
import { log } from "../../../shared/log";
import type { Playlist, SyncBatch } from "../../../shared/types";
import { useRequestScope } from "../../../shared/hooks/useRequestScope";

type Attempt = {
  playlist: Playlist;
  batch: SyncBatch;
  acknowledged: boolean;
};
type Owner = {
  selected: string;
  attempt: Attempt | null;
  pending: boolean;
  completed: boolean;
  unavailable: boolean;
  navigation: (() => void) | null;
  error: string;
};
type State = {
  owner: Owner;
  selected: string;
  pending: boolean;
  attempt: Attempt | null;
  unavailable: boolean;
  error: string;
};

export function useAudiobookGrouping(
  account: string,
  playlists: Playlist[],
  books: Playlist[],
) {
  const scope = useRequestScope(account);
  const owner = useMemo<Owner>(
    () => ({
      selected: "",
      attempt: null,
      pending: false,
      completed: false,
      unavailable: false,
      navigation: null,
      error: "",
    }),
    [account],
  );
  const [state, setState] = useState<State | null>(null);
  const current = state?.owner === owner ? state : null;
  const grouped = new Set(books.map((book) => book.id));
  const eligible = playlists.filter(
    (playlist) => playlist.kind === "local" && !grouped.has(playlist.id),
  );

  function publish() {
    setState({
      owner,
      selected: owner.selected,
      pending: owner.pending || owner.navigation !== null,
      attempt: owner.attempt,
      unavailable: owner.unavailable,
      error: owner.error,
    });
  }

  function navigate(
    attempt: Attempt,
    request: ReturnType<typeof scope.capture>,
  ) {
    let token: (() => void) | null = null;
    let closed = false;
    let outcome = "pending";
    const started = performance.now();
    const fields = {
      request_id: attempt.batch.operations[0].id,
      action: "audiobooks.navigate",
    };
    const active = () =>
      !closed && request.active() && owner.navigation === cancel;
    const release = () => {
      closed = true;
      request.signal.removeEventListener("abort", cancel);
      if (owner.navigation === cancel) {
        owner.navigation = null;
        if (request.active()) publish();
      }
      log.debug("audiobooks.navigation_finished", {
        ...fields,
        reason: outcome,
        duration_ms: Math.round(performance.now() - started),
      });
    };
    const cancel = () => {
      if (closed) return;
      if (outcome === "pending") outcome = "retired";
      release();
      token?.();
    };
    const failed = (reason: string) => {
      if (!active() || outcome !== "pending") return;
      outcome = reason;
      owner.completed = false;
      owner.error =
        "Hito could not open the audiobook page. Check audiobook to try again.";
      publish();
      log.warn("audiobooks.navigation_failed", {
        ...fields,
        reason,
        duration_ms: Math.round(performance.now() - started),
      });
    };
    owner.navigation = cancel;
    request.signal.addEventListener("abort", cancel, { once: true });
    publish();
    log.debug("audiobooks.navigation_started", fields);
    try {
      router.visit(`/audiobooks/${encodeURIComponent(attempt.playlist.id)}`, {
        onBefore: active,
        onCancelToken: (value) => {
          token = value.cancel;
          if (closed || !request.active()) value.cancel();
        },
        onSuccess: (page) => {
          if (!active() || outcome !== "pending") return;
          const props = page.props;
          if (
            page.component !== "AudiobookPage" ||
            !record(props) ||
            !record(props.user) ||
            props.user.id !== account ||
            !record(props.detail) ||
            !record(props.detail.playlist) ||
            props.detail.playlist.id !== attempt.playlist.id ||
            props.detail.playlist.kind !== "local"
          ) {
            failed("response");
            return;
          }
          outcome = "opened";
          owner.completed = true;
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
        },
      });
    } catch {
      failed("request");
      cancel();
    }
  }

  async function perform(attempt: Attempt): Promise<void> {
    const request = scope.capture();
    if (
      !request.active() ||
      owner.pending ||
      owner.navigation ||
      owner.completed
    )
      return;
    owner.pending = true;
    owner.error = "";
    owner.unavailable = false;
    publish();
    const started = performance.now();
    const fields = {
      request_id: attempt.batch.operations[0].id,
      action: "audiobooks.group",
    };
    log.info("audiobooks.group_started", {
      ...fields,
      mode: attempt.acknowledged ? "read" : "write",
    });
    let outcome = "cancelled";
    try {
      if (!attempt.acknowledged) {
        await submitChange(account, attempt.batch, request.signal);
        if (!request.active()) return;
        attempt.acknowledged = true;
        publish();
      }
      await audiobookApi.read(account, attempt.playlist.id, request.signal);
      if (!request.active()) return;
      outcome = "confirmed";
      navigate(attempt, request);
    } catch (error) {
      if (!request.active()) return;
      if (attempt.acknowledged) {
        owner.unavailable = error instanceof HttpError && error.status === 404;
        outcome = owner.unavailable ? "unavailable" : "read_failed";
        owner.error = owner.unavailable
          ? "This audiobook is no longer available. Its playlist may have been deleted."
          : "Hito could not open the audiobook. Check again to recover its result.";
      } else {
        const rejected =
          error instanceof HttpError &&
          [400, 401, 403, 404, 409, 413, 415, 422].includes(error.status);
        if (rejected) owner.attempt = null;
        outcome = rejected ? "rejected" : "unconfirmed";
        owner.error = rejected
          ? errorMessage(error)
          : "Hito could not confirm the audiobook. Retry adding it to recover its result.";
      }
    } finally {
      owner.pending = false;
      if (request.active()) publish();
      const fieldsWithOutcome = {
        ...fields,
        reason: outcome,
        duration_ms: Math.round(performance.now() - started),
      };
      if (outcome === "confirmed")
        log.info("audiobooks.group_finished", fieldsWithOutcome);
      else if (outcome === "cancelled")
        log.debug("audiobooks.group_finished", fieldsWithOutcome);
      else log.warn("audiobooks.group_finished", fieldsWithOutcome);
    }
  }

  const attempt = current?.attempt ?? null;
  const selected = current?.selected ?? "";
  const choices =
    attempt && !eligible.some((playlist) => playlist.id === attempt.playlist.id)
      ? [...eligible, attempt.playlist]
      : eligible;
  return {
    playlists: choices,
    selected: choices.some((playlist) => playlist.id === selected)
      ? selected
      : "",
    pending: current?.pending ?? false,
    hasAttempt: attempt !== null,
    acknowledged: attempt?.acknowledged ?? false,
    canChooseAnother:
      attempt?.acknowledged === true &&
      current?.unavailable === true &&
      current?.pending === false,
    error: current?.error ?? "",
    select: (id: string) => {
      if (!scope.capture().active() || owner.pending || owner.attempt) return;
      owner.selected = eligible.some((playlist) => playlist.id === id)
        ? id
        : "";
      owner.error = "";
      publish();
    },
    add: async () => {
      if (!scope.capture().active() || owner.pending || owner.attempt) return;
      const playlist = eligible.find(
        (playlist) => playlist.id === owner.selected,
      );
      if (!playlist) return;
      const batch = prepareChange({
        kind: "audiobook_create",
        playlist_id: playlist.id,
      });
      Object.freeze(batch.operations[0].action);
      Object.freeze(batch.operations[0]);
      Object.freeze(batch.operations);
      Object.freeze(batch);
      owner.attempt = { playlist: { ...playlist }, batch, acknowledged: false };
      await perform(owner.attempt);
    },
    retry: async () => {
      if (owner.attempt) await perform(owner.attempt);
    },
    chooseAnother: () => {
      if (
        !scope.capture().active() ||
        owner.pending ||
        owner.navigation ||
        !owner.attempt?.acknowledged ||
        !owner.unavailable
      )
        return;
      const started = performance.now();
      const requestId = owner.attempt.batch.operations[0].id;
      owner.attempt = null;
      owner.selected = "";
      owner.error = "";
      owner.completed = false;
      owner.unavailable = false;
      publish();
      log.info("audiobooks.group_released", {
        request_id: requestId,
        action: "audiobooks.group",
        reason: "unavailable",
        duration_ms: Math.round(performance.now() - started),
      });
    },
  };
}

function record(value: unknown): value is Record<string, unknown> {
  return value !== null && typeof value === "object" && !Array.isArray(value);
}
