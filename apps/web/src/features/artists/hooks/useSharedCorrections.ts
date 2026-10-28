import { useEffect, useState } from "react";
import { artistCorrectionApi } from "../api/artistCorrectionApi";
import { createUuid } from "../../../shared/uuid";
import { useRequestScope } from "../../../shared/hooks/useRequestScope";
import { useRetainedCommand } from "./useRetainedCommand";
import type {
  SharedArtistCorrection,
  SharedArtistCorrections,
} from "../../../shared/types";

const pageSize = 25;

/** Lists the corrections shared with every account and lets an administrator withdraw or restore them. */
export function useSharedCorrections(account: string) {
  const scope = useRequestScope(account);
  const [offset, setOffset] = useState(0);
  const [page, setPage] = useState<SharedArtistCorrections | null>(null);
  const [failed, setFailed] = useState(false);

  async function load(signal: AbortSignal, at = offset) {
    const next = await artistCorrectionApi.shared(account, signal, at, pageSize);
    signal.throwIfAborted();
    setPage(next);
    setFailed(false);
  }

  useEffect(() => {
    const captured = scope.capture();
    load(captured.signal).catch(() => {
      if (captured.active()) setFailed(true);
    });
  }, [scope, offset]);

  const command = useRetainedCommand(account, (signal) => load(signal), () => {});
  return {
    page,
    failed,
    offset,
    pageSize,
    pending: command.pending,
    retrying: command.retrying,
    error: command.error,
    notice: command.notice,
    retry: command.retry,
    previous: () => setOffset((value) => Math.max(0, value - pageSize)),
    next: () => setOffset((value) => value + pageSize),
    toggle: (item: SharedArtistCorrection) =>
      command.run(item.applied ? "withdraw" : "restore", () => {
        const body = Object.freeze({
          request_id: createUuid(),
          expected_revision: item.revision,
        });
        return {
          kind: item.applied ? "withdraw" : "restore",
          requestId: body.request_id,
          notice: item.applied
            ? "The shared correction was withdrawn."
            : "The shared correction was restored.",
          send: (signal) =>
            artistCorrectionApi.undoShared(account, item.id, body, signal),
        };
      }),
  };
}
