import type { Schema } from "../../../shared/schema";
import { useRef, useState } from "react";
import { errorMessage } from "../../../shared/format";
import { HttpError } from "../../../shared/http";
import { log } from "../../../shared/log";

import { createUuid } from "../../../shared/uuid";
import { useRequestScope } from "../../../shared/hooks/useRequestScope";
import { albumEditionChoiceApi } from "../api/albumEditionChoiceApi";

const REQUEST_DEADLINE_MS = 15_000;
const SAVE_FAILED =
  "This edition could not be remembered. Try again or reload the album.";
const CHANGED =
  "The remembered edition changed elsewhere. It was reloaded. Try again.";
const CHANGED_NOT_RELOADED =
  "The remembered edition changed elsewhere. Reload the album, then try again.";

type Command = {
  action: string;
  edition: Schema<"SavedAlbumEdition"> | null;
  send: (signal: AbortSignal) => Promise<Schema<"AlbumEditionChoiceReceipt">>;
};
type Intent = { edition: Schema<"SavedAlbumEdition"> | null };
type Reload = "reloaded" | "unavailable";
type Outcome = "accepted" | "unknown" | "conflict" | "rejected" | "stopped";

function fromReceipt(
  receipt: Schema<"AlbumEditionChoiceReceipt">,
): Schema<"AlbumEditionChoice"> {
  return {
    revision: receipt.revision,
    state: receipt.edition
      ? { status: "ready", edition: receipt.edition }
      : { status: "unset" },
  };
}

function same(a: Schema<"SavedAlbumEdition">, b: Schema<"SavedAlbumEdition">) {
  return (
    a.recording_id === b.recording_id &&
    a.association_id === b.association_id &&
    a.release_group_id === b.release_group_id &&
    a.edition_id === b.edition_id
  );
}

export function useAlbumEditionChoice(
  account: string,
  release: string,
  initial: Schema<"AlbumEditionChoice">,
) {
  const scope = useRequestScope(`${account}/${release}`);
  const [choice, setChoice] = useState(initial);
  const [remembered, setRemembered] =
    useState<Schema<"SavedAlbumEdition"> | null>(null);
  const [pending, setPending] = useState(false);
  const [error, setError] = useState("");
  const busy = useRef(false);
  const revision = useRef(initial.revision);
  const saved = useRef(initial.state);
  const wanted = useRef<Schema<"SavedAlbumEdition"> | null>(null);
  const unacknowledged = useRef<Command | null>(null);
  const intent = useRef<Intent | null>(null);

  function apply(next: Schema<"AlbumEditionChoice">) {
    revision.current = next.revision;
    saved.current = next.state;
    setChoice(next);
  }

  async function reloadChoice(signal: AbortSignal): Promise<Reload> {
    try {
      apply(await albumEditionChoiceApi.current(account, release, signal));
      return "reloaded";
    } catch {
      return "unavailable";
    }
  }

  async function execute(command: Command): Promise<Outcome> {
    const request = scope.capture();
    const started = performance.now();
    const controller = new AbortController();
    const cancel = () => controller.abort();
    request.signal.addEventListener("abort", cancel, { once: true });
    const timer = setTimeout(() => controller.abort(), REQUEST_DEADLINE_MS);
    let outcome: Outcome = "accepted";
    try {
      const receipt = await command.send(controller.signal);
      if (!request.active()) return (outcome = "stopped");
      apply(fromReceipt(receipt));
      unacknowledged.current = null;
      return outcome;
    } catch (failure) {
      if (!request.active()) return (outcome = "stopped");
      if (failure instanceof HttpError) {
        unacknowledged.current = null;
        if (failure.status === 409) {
          outcome = "conflict";
          const reload = await reloadChoice(controller.signal);
          setError(reload === "reloaded" ? CHANGED : CHANGED_NOT_RELOADED);
        } else {
          outcome = "rejected";
          setError(errorMessage(failure));
        }
      } else {
        outcome = "unknown";
        unacknowledged.current = command;
        setError(`${errorMessage(failure)} Try again to confirm the change.`);
      }
      return outcome;
    } finally {
      clearTimeout(timer);
      request.signal.removeEventListener("abort", cancel);
      const record =
        outcome === "accepted" || outcome === "stopped" ? log.debug : log.warn;
      record(`${command.action}.finished`, {
        reason: outcome,
        duration_ms: Math.round(performance.now() - started),
      });
    }
  }

  async function drain(): Promise<boolean> {
    if (busy.current) return false;
    busy.current = true;
    setPending(true);
    setError("");
    try {
      for (;;) {
        const lost = unacknowledged.current;
        if (lost) {
          if ((await execute(lost)) !== "accepted") return false;
        }
        const target = wanted.current;
        if (!target) return true;
        const state = saved.current;
        if (state.status === "ready" && same(state.edition, target)) {
          wanted.current = null;
          setRemembered(target);
          return true;
        }
        const body = {
          request_id: createUuid(),
          expected_revision: revision.current,
        };
        const outcome = await execute({
          action: "album_edition.choose",
          edition: target,
          send: (signal) =>
            albumEditionChoiceApi.choose(
              account,
              release,
              { ...body, ...target },
              signal,
            ),
        });
        if (outcome === "accepted") {
          if (wanted.current === target) {
            wanted.current = null;
            setRemembered(target);
            return true;
          }
          continue;
        }
        if (outcome === "conflict" || outcome === "rejected")
          setError((message) => message || SAVE_FAILED);
        return false;
      }
    } finally {
      busy.current = false;
      if (scope.capture().active()) setPending(false);
    }
  }

  function choose(edition: Schema<"SavedAlbumEdition">) {
    intent.current = { edition };
    wanted.current = edition;
    setRemembered(null);
    return drain();
  }

  async function forget() {
    intent.current = { edition: null };
    wanted.current = null;
    setRemembered(null);
    if (busy.current) return false;
    busy.current = true;
    setPending(true);
    setError("");
    try {
      const lost = unacknowledged.current;
      if (lost) {
        const outcome = await execute(lost);
        if (outcome !== "accepted") return false;
      }
      const body = {
        request_id: createUuid(),
        expected_revision: revision.current,
      };
      const outcome = await execute({
        action: "album_edition.clear",
        edition: null,
        send: (signal) =>
          albumEditionChoiceApi.clear(account, release, body, signal),
      });
      return outcome === "accepted";
    } finally {
      busy.current = false;
      if (scope.capture().active()) setPending(false);
    }
  }

  function retry() {
    const last = intent.current;
    if (!last) return Promise.resolve(false);
    return last.edition ? choose(last.edition) : forget();
  }

  function reading() {
    setRemembered(null);
  }

  function stale() {
    setRemembered(null);
    saved.current = { status: "needs_choice" };
    setChoice((current) => ({ ...current, state: { status: "needs_choice" } }));
  }

  return {
    choice,
    remembered,
    pending,
    error,
    choose,
    forget,
    retry,
    reading,
    stale,
  };
}
