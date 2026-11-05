import type { Schema } from "../../../shared/schema";
import { requestJson } from "../../../shared/http";

export const historyApi = {
  preference(
    account: string,
    signal: AbortSignal,
  ): Promise<Schema<"HistoryPreference">> {
    return requestJson<Schema<"HistoryPreference">>("/history/preferences", {
      action: "history.preference",
      account,
      signal,
      cache: "no-store",
    });
  },
  setEnabled(
    account: string,
    enabled: boolean,
    signal: AbortSignal,
  ): Promise<Schema<"HistoryPreference">> {
    const body: Schema<"UpdateHistoryPreference"> = { enabled };
    return requestJson<Schema<"HistoryPreference">>("/history/preferences", {
      action: "history.preference.update",
      method: "PUT",
      account,
      body,
      signal,
    });
  },
  clear(
    account: string,
    signal: AbortSignal,
  ): Promise<Schema<"HistoryCleared">> {
    return requestJson<Schema<"HistoryCleared">>("/history", {
      action: "history.delete",
      method: "DELETE",
      account,
      signal,
    });
  },
};
