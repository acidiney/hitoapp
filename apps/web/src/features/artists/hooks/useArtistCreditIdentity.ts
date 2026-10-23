import { useEffect, useMemo, useState } from "react";
import { artistIdentityApi } from "../api/artistIdentityApi";
import { HttpError } from "../../../shared/http";
import { log } from "../../../shared/log";
import { createUuid } from "../../../shared/uuid";
import type {
  ApplyArtistIdentityToCredit,
  ArtistIdentityCreditReceipt,
  ArtistIdentityGroup,
} from "../../../shared/types";

export type CreditIdentityTotals = {
  applied: number;
  unchanged: number;
  skippedDifferent: number;
  skippedUnavailable: number;
};
type Step = "closed" | "loading" | "choosing" | "confirming" | "applying";
type Owner = {
  controller: AbortController | null;
  attempt: ApplyArtistIdentityToCredit | null;
  totals: CreditIdentityTotals;
};
type View = {
  step: Step;
  groups: ArtistIdentityGroup[];
  selected: ArtistIdentityGroup | null;
  totals: CreditIdentityTotals | null;
  error: string;
  retrying: boolean;
};

const empty: View = {
  step: "closed",
  groups: [],
  selected: null,
  totals: null,
  error: "",
  retrying: false,
};
const noTotals = (): CreditIdentityTotals => ({
  applied: 0,
  unchanged: 0,
  skippedDifferent: 0,
  skippedUnavailable: 0,
});
const DEADLINE_MS = 15000;

function validReceipt(
  receipt: ArtistIdentityCreditReceipt,
  artist: string,
  body: ApplyArtistIdentityToCredit,
) {
  return (
    !!receipt &&
    receipt.request_id === body.request_id &&
    receipt.artist_id === artist &&
    receipt.group_id === body.group_id &&
    [
      receipt.applied,
      receipt.unchanged,
      receipt.skipped_different,
      receipt.skipped_unavailable,
    ].every((count) => Number.isSafeInteger(count) && count >= 0) &&
    (receipt.next_after === undefined ||
      receipt.next_after === null ||
      (typeof receipt.next_after === "string" && receipt.next_after.length > 0))
  );
}

export function useArtistCreditIdentity(
  account: string,
  artist: { id: string; name: string },
) {
  const owner = useMemo<Owner>(
    () => ({ controller: null, attempt: null, totals: noTotals() }),
    [account, artist.id],
  );
  const [view, setView] = useState<View>(empty);
  useEffect(() => {
    setView(empty);
    return () => owner.controller?.abort();
  }, [owner]);

  async function run<T>(work: (signal: AbortSignal) => Promise<T>) {
    if (owner.controller) return undefined;
    const controller = new AbortController();
    owner.controller = controller;
    const timer = setTimeout(() => controller.abort(), DEADLINE_MS);
    try {
      return await work(controller.signal);
    } finally {
      clearTimeout(timer);
      if (owner.controller === controller) owner.controller = null;
    }
  }

  async function find() {
    setView({ ...empty, step: "loading" });
    try {
      const page = await run((signal) =>
        artistIdentityApi.groupsForCredit(account, artist.name, signal),
      );
      setView(
        page ? { ...empty, step: "choosing", groups: page.items } : empty,
      );
    } catch {
      setView({
        ...empty,
        error: "Hito could not load reviewed artists. Try again.",
      });
    }
  }

  async function apply(group: ArtistIdentityGroup) {
    if (owner.controller) return;
    owner.totals = noTotals();
    owner.attempt = {
      request_id: createUuid(),
      group_id: group.id,
    };
    await continueApply(group);
  }

  async function continueApply(group: ArtistIdentityGroup) {
    const started = performance.now();
    let reason = "completed";
    setView((value) => ({
      ...value,
      step: "applying",
      selected: group,
      error: "",
      retrying: false,
    }));
    try {
      while (owner.attempt) {
        const body = owner.attempt;
        const receipt = await run((signal) =>
          artistIdentityApi.applyToCredit(account, artist.id, body, signal),
        );
        if (!receipt) return;
        if (!validReceipt(receipt, artist.id, body))
          throw new Error("Invalid artist identity receipt");
        owner.totals = {
          applied: owner.totals.applied + receipt.applied,
          unchanged: owner.totals.unchanged + receipt.unchanged,
          skippedDifferent:
            owner.totals.skippedDifferent + receipt.skipped_different,
          skippedUnavailable:
            owner.totals.skippedUnavailable + receipt.skipped_unavailable,
        };
        owner.attempt = receipt.next_after
          ? {
              request_id: createUuid(),
              group_id: group.id,
              after: receipt.next_after,
            }
          : null;
      }
      setView({ ...empty, totals: owner.totals, selected: group });
    } catch (error) {
      reason = "failed";
      const rejected =
        error instanceof HttpError && error.status >= 400 && error.status < 500;
      if (rejected) owner.attempt = null;
      setView((value) => ({
        ...value,
        step: "choosing",
        totals: null,
        retrying: owner.attempt !== null,
        error: owner.attempt
          ? "Hito could not confirm the artist identity request. Retry the same request to recover its result."
          : "Hito could not apply this artist identity. Check the credit and try again.",
      }));
    } finally {
      log.info("artist_identity_credit.finished", {
        reason,
        count: owner.totals.applied,
        duration_ms: Math.round(performance.now() - started),
      });
    }
  }

  return {
    ...view,
    find,
    choose: (group: ArtistIdentityGroup) =>
      setView((value) => ({
        ...value,
        step: "confirming",
        selected: group,
        error: "",
      })),
    cancel: () =>
      setView((value) => ({ ...value, step: "choosing", selected: null })),
    apply,
    retry: () => (view.selected ? continueApply(view.selected) : undefined),
  };
}
