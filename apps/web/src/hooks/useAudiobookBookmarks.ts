import { useEffect, useMemo, useState } from "react";
import { prepareChange, submitChange } from "../api";
import { audiobookApi } from "../audiobookApi";
import { HttpError } from "../shared/http";
import { log } from "../shared/log";
import type { Action, AudiobookDetail, SyncBatch } from "../shared/types";
import { createUuid } from "../shared/uuid";
import { useRequestScope } from "../shared/hooks/useRequestScope";

type BookmarkAction = Extract<
  Action,
  { kind: "audiobook_bookmark_create" | "audiobook_bookmark_remove" }
>;
type Draft = { entry: string; position: string; label: string };
type Attempt = {
  action: BookmarkAction;
  batch: SyncBatch;
  acknowledged: boolean;
};
type ReadRun = { controller: AbortController; revision: number };
type Owner = {
  rendered: AudiobookDetail;
  detail: AudiobookDetail;
  draft: Draft;
  attempt: Attempt | null;
  pending: boolean;
  verified: boolean;
  revision: number;
  needsValidation: boolean;
  validation: ReadRun | null;
  validationError: boolean;
  error: string;
  message: string;
};

export function useAudiobookBookmarks(
  account: string,
  rendered: AudiobookDetail,
) {
  const book = rendered.playlist.id;
  const scope = useRequestScope(JSON.stringify([account, book]));
  const owner = useMemo<Owner>(
    () => ({
      rendered,
      detail: rendered,
      draft: { entry: "", position: "", label: "" },
      attempt: null,
      pending: false,
      verified: false,
      revision: 0,
      needsValidation: false,
      validation: null,
      validationError: false,
      error: "",
      message: "",
    }),
    [account, book],
  );
  const [revision, setRevision] = useState(0);
  const publish = () => setRevision((revision) => revision + 1);
  if (owner.rendered !== rendered) {
    owner.rendered = rendered;
    owner.revision += 1;
    if (owner.verified || owner.attempt || owner.pending || owner.validation)
      owner.needsValidation = true;
    else owner.detail = rendered;
  }

  async function boundedRequest<T>(
    signal: AbortSignal,
    operation: (signal: AbortSignal) => Promise<T>,
  ): Promise<T> {
    signal.throwIfAborted();
    const controller = new AbortController();
    const cancel = () => controller.abort();
    signal.addEventListener("abort", cancel, { once: true });
    const deadline = setTimeout(cancel, 10_000);
    try {
      return await operation(controller.signal);
    } finally {
      clearTimeout(deadline);
      signal.removeEventListener("abort", cancel);
    }
  }

  async function refresh(): Promise<void> {
    const request = scope.capture();
    if (!request.active() || owner.pending || owner.attempt || owner.validation)
      return;
    const run = {
      controller: new AbortController(),
      revision: owner.revision,
    };
    const active = () =>
      request.active() &&
      owner.validation === run &&
      owner.revision === run.revision;
    const cancel = () => run.controller.abort();
    request.signal.addEventListener("abort", cancel, { once: true });
    owner.validation = run;
    owner.needsValidation = false;
    owner.validationError = false;
    owner.error = "";
    owner.message = "";
    publish();
    const started = performance.now();
    const fields = {
      request_id: createUuid(),
      action: "audiobook_bookmarks_read",
    };
    log.debug("audiobooks.bookmarks_refresh_started", fields);
    let outcome = "cancelled";
    try {
      const detail = await boundedRequest(run.controller.signal, (signal) =>
        audiobookApi.read(account, book, signal),
      );
      if (!active()) return;
      owner.detail = detail;
      owner.verified = true;
      outcome = "refreshed";
    } catch {
      if (!active()) return;
      owner.validationError = true;
      owner.error =
        "Hito could not refresh bookmarks. Check bookmarks to try again.";
      outcome = "read_failed";
    } finally {
      request.signal.removeEventListener("abort", cancel);
      if (owner.validation === run) {
        owner.validation = null;
        if (request.active()) publish();
      }
      const result = {
        ...fields,
        reason: outcome,
        duration_ms: Math.round(performance.now() - started),
      };
      if (outcome === "read_failed")
        log.warn("audiobooks.bookmarks_refresh_finished", result);
      else log.debug("audiobooks.bookmarks_refresh_finished", result);
    }
  }

  useEffect(() => {
    if (owner.validation && owner.validation.revision !== owner.revision) {
      owner.validation.controller.abort();
      owner.validation = null;
    }
    if (owner.needsValidation && !owner.pending && !owner.attempt)
      void refresh();
  }, [owner, rendered, revision]);

  async function perform(attempt: Attempt): Promise<void> {
    const request = scope.capture();
    if (
      !request.active() ||
      owner.pending ||
      owner.validation ||
      owner.attempt !== attempt
    )
      return;
    owner.pending = true;
    owner.validationError = false;
    owner.error = "";
    owner.message = "";
    publish();
    const started = performance.now();
    const fields = {
      request_id: attempt.batch.operations[0].id,
      action: attempt.action.kind,
    };
    log.info("audiobooks.bookmark_started", {
      ...fields,
      mode: attempt.acknowledged ? "read" : "write",
    });
    let outcome = "cancelled";
    try {
      if (!attempt.acknowledged) {
        await boundedRequest(request.signal, (signal) =>
          submitChange(account, attempt.batch, signal),
        );
        if (!request.active()) return;
        attempt.acknowledged = true;
        publish();
      }
      const readRevision = owner.revision;
      const detail = await boundedRequest(request.signal, (signal) =>
        audiobookApi.read(account, book, signal),
      );
      if (!request.active()) return;
      if (owner.revision !== readRevision)
        throw new Error(
          "The audiobook changed while its bookmark was checked.",
        );
      if (!confirms(detail, attempt.action))
        throw new Error("The bookmark result could not be verified.");
      owner.detail = detail;
      owner.verified = true;
      owner.needsValidation = false;
      owner.attempt = null;
      if (attempt.action.kind === "audiobook_bookmark_create")
        owner.draft = { entry: "", position: "", label: "" };
      owner.message =
        attempt.action.kind === "audiobook_bookmark_create"
          ? "Bookmark saved."
          : "Bookmark removed.";
      outcome = "confirmed";
    } catch (error) {
      if (!request.active()) return;
      if (attempt.acknowledged) {
        outcome = "read_failed";
        owner.error =
          "Hito could not verify the bookmark result. Check bookmarks to recover it.";
      } else {
        const rejected =
          error instanceof HttpError &&
          [400, 401, 403, 404, 409, 413, 415, 422].includes(error.status);
        if (rejected) owner.attempt = null;
        outcome = rejected ? "rejected" : "unconfirmed";
        owner.error = rejected
          ? "Hito could not save this bookmark change. Check its chapter and values, then try again."
          : "Hito could not confirm this bookmark change. Retry it to recover the result.";
      }
    } finally {
      owner.pending = false;
      if (request.active()) publish();
      const result = {
        ...fields,
        reason: outcome,
        duration_ms: Math.round(performance.now() - started),
      };
      if (outcome === "confirmed")
        log.info("audiobooks.bookmark_finished", result);
      else if (outcome === "cancelled")
        log.debug("audiobooks.bookmark_finished", result);
      else log.warn("audiobooks.bookmark_finished", result);
    }
  }

  async function begin(action: BookmarkAction): Promise<void> {
    try {
      const batch = prepareChange(action);
      Object.freeze(action);
      Object.freeze(batch.operations[0]);
      Object.freeze(batch.operations);
      Object.freeze(batch);
      owner.attempt = { action, batch, acknowledged: false };
    } catch {
      owner.error = "Hito could not prepare the bookmark change. Try again.";
      publish();
      log.warn("audiobooks.bookmark_rejected", {
        action: action.kind,
        reason: "prepare_failed",
      });
      return;
    }
    await perform(owner.attempt);
  }

  function reject() {
    owner.error =
      "Choose an available chapter, a nonnegative position with up to three decimal places, and a label of 1 to 256 bytes.";
    owner.message = "";
    publish();
    log.warn("audiobooks.bookmark_rejected", { reason: "invalid_input" });
  }

  return {
    detail: owner.detail,
    draft: owner.draft,
    pending: owner.pending || owner.validation !== null,
    hasAttempt: owner.attempt !== null,
    acknowledged: owner.attempt?.acknowledged ?? false,
    canCheck: owner.validation !== null || owner.validationError,
    error: owner.error,
    message: owner.message,
    change: (field: keyof Draft, value: string) => {
      if (
        !scope.capture().active() ||
        owner.pending ||
        owner.validation ||
        owner.attempt
      )
        return;
      owner.draft = { ...owner.draft, [field]: value };
      if (!owner.validationError) owner.error = "";
      owner.message = "";
      publish();
    },
    save: async () => {
      if (
        !scope.capture().active() ||
        owner.pending ||
        owner.validation ||
        owner.attempt
      )
        return;
      const chapter = owner.detail.chapters.find(
        (entry) => entry.entry_id === owner.draft.entry,
      );
      const position = milliseconds(owner.draft.position);
      if (
        owner.draft.label.length > 256 ||
        new TextEncoder().encode(owner.draft.label).byteLength > 256
      ) {
        reject();
        return;
      }
      const label = owner.draft.label.trim();
      if (
        !chapter?.recording_id ||
        !chapter.available ||
        position === null ||
        !label
      ) {
        reject();
        return;
      }
      await begin({
        kind: "audiobook_bookmark_create",
        audiobook_id: book,
        bookmark_id: createUuid(),
        entry_id: chapter.entry_id,
        recording_id: chapter.recording_id,
        position_ms: position,
        label,
      });
    },
    remove: async (id: string) => {
      if (
        !scope.capture().active() ||
        owner.pending ||
        owner.validation ||
        owner.attempt
      )
        return;
      if (
        !owner.detail.bookmarks.some(
          (bookmark) => bookmark.id === id && bookmark.audiobook_id === book,
        )
      )
        return;
      await begin({
        kind: "audiobook_bookmark_remove",
        audiobook_id: book,
        bookmark_id: id,
      });
    },
    retry: async () => {
      if (owner.attempt) await perform(owner.attempt);
      else if (owner.validationError) await refresh();
    },
  };
}

function milliseconds(value: string): number | null {
  if (value.length > 20 || !/^\d+(?:\.\d{1,3})?$/.test(value)) return null;
  const [seconds, fraction = ""] = value.split(".");
  const result = Number(seconds) * 1000 + Number(fraction.padEnd(3, "0"));
  return Number.isSafeInteger(result) && result >= 0 ? result : null;
}

function confirms(detail: AudiobookDetail, action: BookmarkAction): boolean {
  const bookmark = detail.bookmarks.find(
    (value) => value.id === action.bookmark_id,
  );
  if (action.kind === "audiobook_bookmark_remove") return !bookmark;
  return (
    bookmark?.audiobook_id === action.audiobook_id &&
    bookmark.entry_id === action.entry_id &&
    bookmark.recording_id === action.recording_id &&
    bookmark.position_ms === action.position_ms &&
    bookmark.label === action.label
  );
}
