import { useState } from "react";

export function lyricQuerySeed(title: string, artist: string): string {
  return [title, artist]
    .map((part) => part.trim())
    .filter((part) => part !== "")
    .join(" ");
}

export function useLyricQuery(scopeKey: string, seed: string) {
  const [draft, setDraft] = useState<{ scopeKey: string; text: string } | null>(
    null,
  );
  const query = draft?.scopeKey === scopeKey ? draft.text : seed;
  return {
    query,
    setQuery: (text: string) => setDraft({ scopeKey, text }),
  };
}
