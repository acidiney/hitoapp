import { useMemo, useState } from "react";
import { errorMessage } from "../../../shared/format";
import { request } from "../../../shared/http";
import { log } from "../../../shared/log";
import type { CommunityCredentialReset } from "../../../shared/types";
import { createUuid } from "../../../shared/uuid";
import { useRequestScope } from "../../../shared/hooks/useRequestScope";

type Owner = { pending: boolean };
type Snapshot = {
  owner: Owner;
  pending: boolean;
  receipt: CommunityCredentialReset | null;
  error: string;
};

function empty(owner: Owner): Snapshot {
  return { owner, pending: false, receipt: null, error: "" };
}

export function useCommunityCredentialReset(account: string) {
  const scope = useRequestScope(account);
  const owner = useMemo<Owner>(() => ({ pending: false }), [scope]);
  const [snapshot, setSnapshot] = useState(() => empty(owner));
  const current = snapshot.owner === owner ? snapshot : empty(owner);

  async function reset() {
    const captured = scope.capture();
    if (!captured.active() || owner.pending) return;
    owner.pending = true;
    setSnapshot({ ...empty(owner), pending: true });
    const requestId = createUuid();
    const started = performance.now();
    let reason = "cancelled";
    try {
      const receipt: CommunityCredentialReset = await (
        await request("/community/credentials", {
          action: "community.credentials.reset",
          method: "DELETE",
          account,
          signal: captured.signal,
        })
      ).json();
      if (!captured.active()) return;
      reason = "accepted";
      owner.pending = false;
      setSnapshot({ owner, pending: false, receipt, error: "" });
    } catch (error) {
      if (!captured.active()) return;
      reason = "failed";
      owner.pending = false;
      setSnapshot({ ...empty(owner), error: errorMessage(error) });
    } finally {
      const record =
        reason === "accepted"
          ? log.info
          : reason === "failed"
            ? log.warn
            : log.debug;
      record("community_credentials.finished", {
        request_id: requestId,
        reason,
        duration_ms: Math.round(performance.now() - started),
      });
    }
  }

  return {
    pending: current.pending,
    receipt: current.receipt,
    error: current.error,
    reset,
  };
}
