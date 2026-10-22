import { useRef, useState } from "react";
import { metadataApi } from "../api/metadataApi";
import { errorMessage } from "../../../shared/format";
import type { ObservationPage } from "../../../shared/types";
import type { MetadataTarget } from "../metadata";
import { useRequestScope } from "../../../shared/hooks/useRequestScope";

export function useMetadataHistory(
  account: string,
  target: MetadataTarget,
  initial: ObservationPage,
) {
  const scope = useRequestScope(`${account}\0${target.kind}\0${target.id}`);
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
      const detail = await metadataApi.history(
        account,
        target,
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
