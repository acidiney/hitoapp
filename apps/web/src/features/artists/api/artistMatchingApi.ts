import { request } from "../../../shared/http";
import type {
  ArtistMatchReceipt,
  ArtistMatchSuggestions,
  DismissArtistMatchSuggestion,
  MatchArtistsFromTags,
} from "../../../shared/types";

const part = encodeURIComponent;

async function json<T>(
  path: string,
  options: {
    action: string;
    account: string;
    signal: AbortSignal;
    method?: "GET" | "POST";
    body?: MatchArtistsFromTags | DismissArtistMatchSuggestion;
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
  run(account: string, body: MatchArtistsFromTags, signal: AbortSignal) {
    return json<ArtistMatchReceipt>("/artist-matching", {
      action: "artist_matching.run",
      account,
      signal,
      method: "POST",
      body,
    });
  },
  suggestions(account: string, artist: string, signal: AbortSignal) {
    return json<ArtistMatchSuggestions>(
      `/artists/${part(artist)}/match-suggestions`,
      { action: "artist_matching.suggestions", account, signal },
    );
  },
  dismiss(
    account: string,
    artist: string,
    body: DismissArtistMatchSuggestion,
    signal: AbortSignal,
  ) {
    return json<ArtistMatchSuggestions>(
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
