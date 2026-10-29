import { useCallback, useState } from "react";
import { change, changeMany } from "../sync";
import { errorMessage } from "../format";
import { useNotice } from "../ui/Notices";
import type { Action } from "../types";
import { useRequestScope } from "./useRequestScope";

export type MutationResult = { ok: true } | { ok: false; error: string };
export type Mutate = (action: Action) => Promise<MutationResult>;
export type MutateMany = (actions: Action[]) => Promise<MutationResult>;

export function useMutation(account: string) {
  const notice = useNotice();
  const scope = useRequestScope(account);
  const [requests, setRequests] = useState({ scope, count: 0 });
  const run = useCallback(
    async (send: (signal: AbortSignal) => Promise<unknown>) => {
      const request = scope.capture();
      setRequests((current) => ({
        scope,
        count: current.scope === scope ? current.count + 1 : 1,
      }));
      try {
        await send(request.signal);
        return request.active()
          ? ({ ok: true } as const)
          : ({
              ok: false,
              error: "This account is no longer active.",
            } as const);
      } catch (error) {
        const message = errorMessage(error);
        if (request.active()) notice(message);
        return { ok: false, error: message } as const;
      } finally {
        if (request.active())
          setRequests((current) => ({
            scope,
            count: Math.max(0, current.count - 1),
          }));
      }
    },
    [notice, scope],
  );
  const mutate: Mutate = useCallback(
    (action) => run((signal) => change(account, action, signal)),
    [account, run],
  );
  const mutateMany: MutateMany = useCallback(
    (actions) => run((signal) => changeMany(account, actions, signal)),
    [account, run],
  );
  return {
    mutate,
    mutateMany,
    pending: requests.scope === scope && requests.count > 0,
  };
}
