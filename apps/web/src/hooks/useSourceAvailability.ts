import { useRef, useState } from "react";
import { libraryApi } from "../api";
import { errorMessage } from "../format";
import type { Source } from "../types";
import { useRequestScope } from "./useRequestScope";

export function useSourceAvailability(account: string, recording: string) {
  const scope = useRequestScope(`${account}\0${recording}`);
  const [state, setState] = useState<{
    scope: typeof scope;
    sources: Source[] | null;
    pending: boolean;
    error: string;
  }>({ scope, sources: null, pending: false, error: "" });
  const busy = useRef<typeof scope | null>(null);
  async function check() {
    if (busy.current === scope) return;
    const request = scope.capture();
    busy.current = scope;
    setState({ scope, sources: null, pending: true, error: "" });
    try {
      const sources = await libraryApi.refreshSources(
        account,
        recording,
        request.signal,
      );
      if (request.active())
        setState({ scope, sources, pending: false, error: "" });
    } catch (error) {
      if (request.active())
        setState({
          scope,
          sources: null,
          pending: false,
          error: errorMessage(error),
        });
    } finally {
      if (busy.current === scope) busy.current = null;
    }
  }
  return {
    sources: state.scope === scope ? state.sources : null,
    pending: state.scope === scope && state.pending,
    error: state.scope === scope ? state.error : "",
    check,
  };
}
