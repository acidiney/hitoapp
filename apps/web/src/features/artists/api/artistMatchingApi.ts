import type { Schema } from "../../../shared/schema";
import { request } from "../../../shared/http";

const part = encodeURIComponent;

async function json<T>(
  path: string,
  options: {
    action: string;
    account: string;
    signal: AbortSignal;
    method?: "GET" | "POST";
    body?:
      | Schema<"MatchArtistsFromTags">
      | Schema<"MatchArtistsFromProviderEvidence">
      | Schema<"DismissArtistMatchSuggestion">;
  },
): Promise<T> {
  const response = await request(path, options);
  if (options.signal.aborted) {
    await response.body?.cancel();
    options.signal.throwIfAborted();
  }
  return response.json();
}

export const artistMatchingApi = {
  run(
    account: string,
    body: Schema<"MatchArtistsFromTags">,
    signal: AbortSignal,
  ) {
    return json<Schema<"ArtistMatchReceipt">>("/artist-matching", {
      action: "artist_matching.run",
      account,
      signal,
      method: "POST",
      body,
    });
  },
  runProviderEvidence(
    account: string,
    body: Schema<"MatchArtistsFromProviderEvidence">,
    signal: AbortSignal,
  ) {
    return json<Schema<"ArtistProviderMatchReceipt">>(
      "/artist-matching/provider-evidence",
      {
        action: "artist_matching.run_provider_evidence",
        account,
        signal,
        method: "POST",
        body,
      },
    );
  },
  suggestions(account: string, artist: string, signal: AbortSignal) {
    return json<Schema<"ArtistMatchSuggestions">>(
      `/artists/${part(artist)}/match-suggestions`,
      { action: "artist_matching.suggestions", account, signal },
    );
  },
  dismiss(
    account: string,
    artist: string,
    body: Schema<"DismissArtistMatchSuggestion">,
    signal: AbortSignal,
  ) {
    return json<Schema<"ArtistMatchSuggestions">>(
      `/artists/${part(artist)}/match-suggestions/dismiss`,
      {
        action: "artist_matching.dismiss",
        account,
        signal,
        method: "POST",
        body,
      },
    );
  },
};
