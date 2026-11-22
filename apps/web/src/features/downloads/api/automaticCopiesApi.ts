import type { Schema } from "../../../shared/schema";
import { requestJson } from "../../../shared/http";

const part = encodeURIComponent;

export const automaticCopiesApi = {
  status(
    account: string,
    signal: AbortSignal,
  ): Promise<Schema<"AutomaticCopies">> {
    return requestJson<Schema<"AutomaticCopies">>("/account/automatic-copies", {
      action: "automatic_copies",
      account,
      signal,
    });
  },
  setKeep(
    account: string,
    keep: boolean,
    signal: AbortSignal,
  ): Promise<Schema<"AutomaticCopies">> {
    return requestJson<Schema<"AutomaticCopies">>("/account/automatic-copies", {
      action: "automatic_copies.update",
      method: "PUT",
      account,
      body: {
        keep_played_tracks: keep,
      } satisfies Schema<"AutomaticCopyPreference">,
      signal,
    });
  },
  budget(
    account: string,
    user: string,
    signal: AbortSignal,
  ): Promise<Schema<"AutomaticCopyBudget">> {
    return requestJson<Schema<"AutomaticCopyBudget">>(
      `/users/${part(user)}/automatic-copy-budget`,
      { action: "automatic_copies.budget", account, signal },
    );
  },
  setBudget(
    account: string,
    user: string,
    bytes: number,
    signal: AbortSignal,
  ): Promise<Schema<"AutomaticCopyBudget">> {
    return requestJson<Schema<"AutomaticCopyBudget">>(
      `/users/${part(user)}/automatic-copy-budget`,
      {
        action: "automatic_copies.budget.update",
        method: "PUT",
        account,
        body: { budget_bytes: bytes } satisfies Schema<"AutomaticCopyBudget">,
        signal,
      },
    );
  },
};
