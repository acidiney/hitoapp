import { useCallback, useState } from "react";
import { change } from "../sync";
import { errorMessage } from "../format";
import { useNotice } from "../ui/Notices";
import type { Action } from "../types";
import { useRequestScope } from "./useRequestScope";

export type MutationResult = { ok: true } | { ok: false; error: string };
export type Mutate = (action: Action) => Promise<MutationResult>;

export function useMutation(account: string) {
  const notice = useNotice();
  const scope = useRequestScope(account);
  const [requests, setRequests] = useState({ scope, count: 0 });
  const mutate: Mutate = useCallback(
    async (action) => {
      const request = scope.capture();
      setRequests((current) => ({
        scope,
        count: current.scope === scope ? current.count + 1 : 1,
      }));
      try {
        await change(account, action, request.signal);
        return request.active()
          ? { ok: true }
          : { ok: false, error: "This account is no longer active." };
      } catch (error) {
        const message = errorMessage(error);
        if (request.active()) notice(message);
        return { ok: false, error: message };
      } finally {
        if (request.active())
          setRequests((current) => ({
            scope,
            count: Math.max(0, current.count - 1),
          }));
      }
    },
    [account, notice, scope],
  );
  return { mutate, pending: requests.scope === scope && requests.count > 0 };
}
