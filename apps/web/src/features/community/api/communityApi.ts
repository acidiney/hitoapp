import { request, requestJson } from "../../../shared/http";
import type {
  CommunityCredentialReset,
  CommunityPreferences,
  CommunityRankingPreferences,
  UpdateCommunityPreferences,
  UpdateCommunityRankingPreferences,
} from "../../../shared/types";

export const communityApi = {
  preferences(
    account: string,
    signal: AbortSignal,
  ): Promise<CommunityPreferences> {
    return requestJson<CommunityPreferences>("/community/preferences", {
      action: "community.preferences",
      account,
      signal,
    });
  },
  updatePreferences(
    account: string,
    preferences: UpdateCommunityPreferences,
    signal: AbortSignal,
  ): Promise<CommunityPreferences> {
    return requestJson<CommunityPreferences>("/community/preferences", {
      action: "community.preferences.update",
      method: "PUT",
      account,
      body: preferences,
      signal,
    });
  },
  resetCredentials(
    account: string,
    signal: AbortSignal,
  ): Promise<CommunityCredentialReset> {
    return requestJson<CommunityCredentialReset>("/community/credentials", {
      action: "community.credentials.reset",
      method: "DELETE",
      account,
      signal,
    });
  },
  async rankingPreferences(
    account: string,
    update: UpdateCommunityRankingPreferences | null,
    signal: AbortSignal,
  ): Promise<CommunityRankingPreferences> {
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
    return response.json() as Promise<CommunityRankingPreferences>;
  },
};
