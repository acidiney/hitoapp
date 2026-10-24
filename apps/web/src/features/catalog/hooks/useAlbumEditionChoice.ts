import { useRef, useState } from "react";
import { errorMessage } from "../../../shared/format";
import { log } from "../../../shared/log";
import type {
  AlbumEditionChoice,
  AlbumEditionChoiceReceipt,
  SavedAlbumEdition,
} from "../../../shared/types";
import { createUuid } from "../../../shared/uuid";
import { useRequestScope } from "../../../shared/hooks/useRequestScope";
import { albumEditionChoiceApi } from "../api/albumEditionChoiceApi";

const REQUEST_DEADLINE_MS = 15_000;
const SAVE_FAILED =
  "This edition could not be remembered. Reload the album and try again.";

function fromReceipt(receipt: AlbumEditionChoiceReceipt): AlbumEditionChoice {
  return {
    revision: receipt.revision,
    state: receipt.edition
      ? { status: "ready", edition: receipt.edition }
      : { status: "unset" },
  };
}

export function useAlbumEditionChoice(
  account: string,
  release: string,
  initial: AlbumEditionChoice,
) {
  const scope = useRequestScope(`${account}/${release}`);
  const [choice, setChoice] = useState(initial);
  const [remembered, setRemembered] = useState(false);
  const [pending, setPending] = useState(false);
  const [error, setError] = useState("");
  const busy = useRef(false);
  const revision = useRef(initial.revision);

  async function run(
    action: string,
    send: (signal: AbortSignal) => Promise<AlbumEditionChoiceReceipt>,
  ) {
    if (busy.current) return false;
    const request = scope.capture();
    busy.current = true;
    setPending(true);
    setError("");
    const started = performance.now();
    const controller = new AbortController();
    const cancel = () => controller.abort();
    request.signal.addEventListener("abort", cancel, { once: true });
    const timer = setTimeout(() => controller.abort(), REQUEST_DEADLINE_MS);
    let outcome = "accepted";
    try {
      const receipt = await send(controller.signal);
      if (!request.active()) return false;
      revision.current = receipt.revision;
      setChoice(fromReceipt(receipt));
      return true;
    } catch (failure) {
      outcome = request.active() ? "failed" : "cancelled";
      if (request.active()) setError(errorMessage(failure));
      return false;
    } finally {
      clearTimeout(timer);
      request.signal.removeEventListener("abort", cancel);
      busy.current = false;
      if (request.active()) setPending(false);
      const record = outcome === "failed" ? log.warn : log.debug;
      record(`${action}.finished`, {
        reason: outcome,
        duration_ms: Math.round(performance.now() - started),
      });
    }
  }

  async function choose(edition: SavedAlbumEdition) {
    const current = choice.state;
    if (
      current.status === "ready" &&
      current.edition.association_id === edition.association_id &&
      current.edition.recording_id === edition.recording_id &&
      current.edition.release_group_id === edition.release_group_id &&
      current.edition.edition_id === edition.edition_id
    ) {
      setRemembered(true);
      return true;
    }
    const saved = await run("album_edition.choose", (signal) =>
      albumEditionChoiceApi.choose(
        account,
        release,
        {
          request_id: createUuid(),
          expected_revision: revision.current,
          ...edition,
        },
        signal,
      ),
    );
    if (saved) setRemembered(true);
    else setError((message) => (message ? SAVE_FAILED : message));
    return saved;
  }

  async function forget() {
    const cleared = await run("album_edition.clear", (signal) =>
      albumEditionChoiceApi.clear(
        account,
        release,
        { request_id: createUuid(), expected_revision: revision.current },
        signal,
      ),
    );
    if (cleared) setRemembered(false);
    return cleared;
  }

  function stale() {
    setRemembered(false);
    setChoice((current) => ({ ...current, state: { status: "needs_choice" } }));
  }

  return { choice, remembered, pending, error, choose, forget, stale };
}
