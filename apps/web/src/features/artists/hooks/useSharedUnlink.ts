import { artistCorrectionApi } from "../api/artistCorrectionApi";
import { createUuid } from "../../../shared/uuid";
import { useRetainedCommand } from "./useRetainedCommand";
import type {
  ArtistAssociationChanges,
  PromoteArtistCorrection,
} from "../../../shared/types";

/** Lets an administrator share, or withdraw, the unlink they made on this recording. */
export function useSharedUnlink(
  account: string,
  recording: string,
  state: ArtistAssociationChanges | null,
  reload: (signal: AbortSignal) => Promise<void>,
  accepted: () => void,
) {
  const command = useRetainedCommand(
    JSON.stringify([account, recording]),
    reload,
    accepted,
  );
  return {
    pending: command.pending,
    retrying: command.retrying,
    error: command.error,
    notice: command.notice,
    retry: command.retry,
    share: () =>
      command.run("share", () => {
        const association = state?.undo?.association_id;
        if (!state?.shareable || !association) return null;
        const requestId = createUuid();
        let body: PromoteArtistCorrection | null = null;
        return {
          kind: "share",
          requestId,
          notice: "The unlink now applies to every account.",
          send: async (signal) => {
            body ??= Object.freeze({
              request_id: requestId,
              expected_revision: (
                await artistCorrectionApi.shared(account, signal)
              ).revision,
              source: Object.freeze({
                kind: "unlink" as const,
                recording_id: recording,
                association_id: association,
              }),
            });
            return artistCorrectionApi.promote(account, body, signal);
          },
        };
      }),
    withdraw: () =>
      command.run("withdraw", () => {
        const applied = state?.shared;
        if (!applied) return null;
        const body = Object.freeze({
          request_id: createUuid(),
          expected_revision: applied.revision,
        });
        return {
          kind: "withdraw",
          requestId: body.request_id,
          notice: "The shared unlink was withdrawn.",
          send: (signal) =>
            artistCorrectionApi.undoShared(account, applied.id, body, signal),
        };
      }),
  };
}
