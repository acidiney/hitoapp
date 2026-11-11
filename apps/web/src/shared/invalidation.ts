import { useSyncExternalStore } from "react";
import type { Schema } from "./schema";

export type InvalidationScope =
  "sources" | "recording" | "playlist" | "playlists" | "lyrics" | "plugins";

export type Invalidation = { scope: InvalidationScope; id: string | null };

const listeners = new Set<(event: Invalidation) => void>();
const versions = new Map<string, number>();
const keyOf = (scope: InvalidationScope, id: string | null) =>
  `${scope}\u0000${id ?? ""}`;

export function invalidate(scope: InvalidationScope, id: string | null = null) {
  const key = keyOf(scope, id);
  versions.set(key, (versions.get(key) ?? 0) + 1);
  listeners.forEach((listener) => listener({ scope, id }));
}

export function onInvalidated(
  scope: InvalidationScope,
  handler: (id: string | null) => void,
) {
  const listener = (event: Invalidation) => {
    if (event.scope === scope) handler(event.id);
  };
  listeners.add(listener);
  return () => {
    listeners.delete(listener);
  };
}

export function useInvalidationVersion(
  scope: InvalidationScope,
  id: string | null = null,
) {
  return useSyncExternalStore(
    (notify) =>
      onInvalidated(scope, (changed) => {
        if (changed === id || changed === null) notify();
      }),
    () =>
      (versions.get(keyOf(scope, id)) ?? 0) +
      (versions.get(keyOf(scope, null)) ?? 0),
  );
}

export function invalidationsFor(action: Schema<"Action">): Invalidation[] {
  const entity = action as {
    kind: string;
    recording_id?: string;
    playlist_id?: string;
  };
  switch (entity.kind) {
    case "like_set":
    case "recording_title_set":
    case "recording_title_inherit":
    case "recording_artist_set":
    case "recording_artist_inherit":
      return [{ scope: "recording", id: entity.recording_id ?? null }];
    case "release_title_set":
    case "release_title_inherit":
    case "release_artist_set":
    case "release_artist_inherit":
      return [{ scope: "recording", id: null }];
    case "playlist_create":
    case "playlist_delete":
    case "playlist_rename":
      return [{ scope: "playlists", id: null }];
    case "entry_insert":
    case "entry_move":
    case "entry_remove":
      return [
        { scope: "playlist", id: entity.playlist_id ?? null },
        { scope: "playlists", id: null },
      ];
    default:
      return [];
  }
}
