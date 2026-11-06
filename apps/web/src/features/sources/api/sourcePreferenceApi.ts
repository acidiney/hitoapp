import type { Schema } from "../../../shared/schema";
import { requestJson } from "../../../shared/http";
import { log } from "../../../shared/log";

const fallbackByAccount = new Map<string, Promise<boolean>>();

export const sourcePreferenceApi = {
  settings(
    account: string,
    signal: AbortSignal,
  ): Promise<Schema<"SourcePreferences">> {
    return requestJson<Schema<"SourcePreferences">>("/source-preferences", {
      action: "source_preferences",
      account,
      signal,
    });
  },
  async update(
    account: string,
    body: Schema<"UpdateSourcePreferences">,
    signal: AbortSignal,
  ): Promise<Schema<"SourcePreferences">> {
    const saved = await requestJson<Schema<"SourcePreferences">>(
      "/source-preferences",
      {
        action: "source_preferences.update",
        method: "PUT",
        account,
        body,
        signal,
      },
    );
    fallbackByAccount.set(account, Promise.resolve(saved.fallback));
    return saved;
  },
  fallbackEnabled(account: string): Promise<boolean> {
    const known = fallbackByAccount.get(account);
    if (known) return known;
    const loading = sourcePreferenceApi
      .settings(account, AbortSignal.timeout(5000))
      .then(
        (settings) => settings.fallback,
        () => {
          fallbackByAccount.delete(account);
          log.warn("source_preferences.fallback_unreadable");
          return true;
        },
      );
    fallbackByAccount.set(account, loading);
    return loading;
  },
  forget(account: string) {
    fallbackByAccount.delete(account);
  },
};
