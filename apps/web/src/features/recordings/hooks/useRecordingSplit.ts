import { useCallback, useState } from "react";
import { HttpError } from "../../../shared/http";
import { log } from "../../../shared/log";
import { useRequestScope } from "../../../shared/hooks/useRequestScope";
import { recordingCopiesApi } from "../api/recordingCopiesApi";

export function useRecordingSplit(
  account: string,
  recording: string,
  onSeparated: () => void,
) {
  const key = JSON.stringify([account, recording]);
  const scope = useRequestScope(key);
  const [state, setState] = useState({ key, splitting: "", error: "" });
  const current = state.key === key ? state : { key, splitting: "", error: "" };

  const split = useCallback(
    async (copy: string) => {
      const request = scope.capture();
      setState({ key, splitting: copy, error: "" });
      try {
        await recordingCopiesApi.split(account, copy, request.signal);
        if (!request.active()) return;
        log.debug("recording_copies.split", {
          account_id: account,
          recording_id: copy,
        });
        setState({ key, splitting: "", error: "" });
        onSeparated();
      } catch (error) {
        if (!request.active()) return;
        log.warn("recording_copies.split_failed", {
          account_id: account,
          recording_id: copy,
          reason: error instanceof HttpError ? "http" : "request",
          status: error instanceof HttpError ? error.status : undefined,
        });
        setState({
          key,
          splitting: "",
          error: "Hito could not separate these songs. Try again.",
        });
      }
    },
    [account, key, onSeparated, scope],
  );

  return { splitting: current.splitting, error: current.error, split };
}
