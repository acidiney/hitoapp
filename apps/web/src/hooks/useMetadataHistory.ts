import { useRef, useState } from "react";
import { metadataApi } from "../metadataApi";
import { errorMessage } from "../format";
import type { ObservationPage } from "../types";
import { useRequestScope } from "./useRequestScope";

export function useMetadataHistory(
  account: string,
  recording: string,
  initial: ObservationPage,
) {
  const scope = useRequestScope(`${account}\0${recording}`);
  const [state, setState] = useState({ scope, page: initial });
  const [pending, setPending] = useState<typeof scope | null>(null);
  const [failure, setFailure] = useState<{
    scope: typeof scope;
    message: string;
  } | null>(null);
  const busy = useRef<typeof scope | null>(null);
  const page = state.scope === scope ? state.page : initial;
  async function older() {
    if (page.next_before === null || busy.current === scope) return;
    const request = scope.capture();
    busy.current = scope;
    setPending(scope);
    setFailure(null);
    try {
      const detail = await metadataApi.recording(
        account,
        recording,
        page.next_before,
        request.signal,
      );
      if (request.active()) setState({ scope, page: detail.observations });
    } catch (error) {
      if (request.active()) setFailure({ scope, message: errorMessage(error) });
    } finally {
      if (busy.current === scope) busy.current = null;
      if (request.active()) setPending(null);
    }
  }
  return {
    page,
    pending: pending === scope,
    error: failure?.scope === scope ? failure.message : "",
    older,
    latest: () => {
      setState({ scope, page: initial });
      setFailure(null);
    },
    isLatest: page === initial,
  };
}
