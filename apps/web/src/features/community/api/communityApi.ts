import type { Schema } from "../../../shared/schema";
import { request, requestJson } from "../../../shared/http";

export const communityApi = {
  preferences(
    account: string,
    signal: AbortSignal,
  ): Promise<Schema<"CommunityPreferences">> {
    return requestJson<Schema<"CommunityPreferences">>(
      "/community/preferences",
      {
        action: "community.preferences",
        account,
        signal,
      },
    );
  },
  updatePreferences(
    account: string,
    preferences: Schema<"UpdateCommunityPreferences">,
    signal: AbortSignal,
  ): Promise<Schema<"CommunityPreferences">> {
    return requestJson<Schema<"CommunityPreferences">>(
      "/community/preferences",
      {
        action: "community.preferences.update",
        method: "PUT",
        account,
        body: preferences,
        signal,
      },
    );
  },
  resetCredentials(
    account: string,
    signal: AbortSignal,
  ): Promise<Schema<"CommunityCredentialReset">> {
    return requestJson<Schema<"CommunityCredentialReset">>(
      "/community/credentials",
      {
        action: "community.credentials.reset",
        method: "DELETE",
        account,
        signal,
      },
    );
  },
  async rankingPreferences(
    account: string,
    update: Schema<"UpdateCommunityRankingPreferences"> | null,
    signal: AbortSignal,
  ): Promise<Schema<"CommunityRankingPreferences">> {
    const response = await request("/community/ranking-preferences", {
      action: update
        ? "community.rankings.preferences.update"
        : "community.rankings.preferences",
      account,
      method: update ? "PUT" : "GET",
      body: update ?? undefined,
      signal,
    });
    if (signal.aborted) {
      await response.body?.cancel();
      signal.throwIfAborted();
    }
    return response.json() as Promise<Schema<"CommunityRankingPreferences">>;
  },
};
