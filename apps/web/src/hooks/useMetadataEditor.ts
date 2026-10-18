import { useRef, useState } from "react";
import { router } from "@inertiajs/react";
import {
  metadataAction,
  type MetadataField,
  type MetadataEditorInput,
} from "../metadata";
import { usePlayer } from "../player/PlaybackProvider";
import { log } from "../log";
import { useMutation, type MutationResult } from "./useMutation";
import { useRequestScope } from "./useRequestScope";

export function useMetadataEditor(
  account: string,
  target: MetadataEditorInput,
) {
  const { mutate } = useMutation(account);
  const player = usePlayer();
  const scope = useRequestScope(`${account}\0${target.kind}\0${target.id}`);
  const [pendingScope, setPendingScope] = useState<typeof scope | null>(null);
  const busy = useRef<typeof scope | null>(null);

  async function save(
    field: MetadataField,
    personal: string | null,
  ): Promise<MutationResult> {
    if (busy.current === scope)
      return { ok: false, error: "Wait for the current change to finish." };
    const request = scope.capture();
    busy.current = scope;
    setPendingScope(scope);
    let started = false;
    let finished = false;
    let cancelReload: (() => void) | undefined;
    const abortReload = () => cancelReload?.();
    function finish() {
      if (finished) return;
      finished = true;
      request.signal.removeEventListener("abort", abortReload);
      if (busy.current === scope) busy.current = null;
      if (request.active()) setPendingScope(null);
      if (started)
        log.debug("metadata.refresh_finished", {
          account_id: account,
          action: target.kind,
        });
    }
    try {
      const result = await mutate(metadataAction(target, field, personal));
      if (!request.active())
        return {
          ok: false,
          error: "This metadata editor is no longer active.",
        };
      if (!result.ok) return result;
      if (target.kind === "recording")
        player.setRecordingField(
          target.id,
          field,
          personal,
          target.metadata[field].inherited,
        );
      request.signal.addEventListener("abort", abortReload, { once: true });
      router.reload({
        onCancelToken: (token) => {
          started = true;
          cancelReload = token.cancel;
          log.debug("metadata.refresh_started", {
            account_id: account,
            action: target.kind,
          });
          if (!request.active()) abortReload();
        },
        onFinish: finish,
      });
      return result;
    } catch {
      abortReload();
      finish();
      log.warn("metadata.refresh_failed", {
        account_id: account,
        action: target.kind,
      });
      return {
        ok: false,
        error: "Your change was saved. Reload the page to refresh its details.",
      };
    } finally {
      if (!started) finish();
    }
  }
  return { save, pending: pendingScope === scope };
}
