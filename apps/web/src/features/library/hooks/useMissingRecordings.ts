import type { Schema } from "../../../shared/schema";
import { useRef, useState } from "react";
import { storageApi } from "../api/storageApi";
import { errorMessage } from "../../../shared/format";
import { useRequestScope } from "../../../shared/hooks/useRequestScope";
import { log } from "../../../shared/log";

export function useMissingRecordings(account: string, library: string) {
  const scope = useRequestScope(`${account}\0${library}`);
  const [state, setState] = useState<{
    scope: typeof scope;
    removing: boolean;
    result: Schema<"MissingRecordingsRemoval"> | null;
    error: string;
  } | null>(null);
  const busy = useRef<typeof scope | null>(null);
  async function remove() {
    if (busy.current === scope) return;
    const request = scope.capture();
    busy.current = scope;
    setState({ scope, removing: true, result: null, error: "" });
    try {
      const result = await storageApi.removeMissingRecordings(
        account,
        library,
        request.signal,
      );
      if (!request.active()) return;
      log.info("recordings.missing_removed", {
        account_id: account,
        library_id: library,
      });
      setState({ scope, removing: false, result, error: "" });
    } catch (error) {
      if (request.active())
        setState({
          scope,
          removing: false,
          result: null,
          error: errorMessage(error),
        });
    } finally {
      if (busy.current === scope) busy.current = null;
    }
  }
  const current = state?.scope === scope ? state : null;
  return {
    removing: current?.removing ?? false,
    result: current?.result ?? null,
    error: current?.error ?? "",
    remove,
  };
}
