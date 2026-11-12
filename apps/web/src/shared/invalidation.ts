import { useSyncExternalStore } from "react";
import type { Schema } from "./schema";

export type InvalidationScope =
  | "sources"
  | "resolved"
  | "recording"
  | "playlist"
  | "playlists"
  | "artwork"
  | "lyrics"
  | "plugins"
  | "catalog";

export type RecordingChange =
  | { liked: boolean }
  | { field: "title" | "artist"; value: string };

export type Invalidation = {
  scope: InvalidationScope;
  id: string | null;
  change?: RecordingChange;
};

const listeners = new Set<(event: Invalidation) => void>();
const versions = new Map<string, number>();
const keyOf = (scope: InvalidationScope, id: string | null) =>
  `${scope}\u0000${id ?? ""}`;

export function invalidate(
  scope: InvalidationScope,
  id: string | null = null,
  change?: RecordingChange,
) {
  const key = keyOf(scope, id);
  versions.set(key, (versions.get(key) ?? 0) + 1);
  listeners.forEach((listener) => listener({ scope, id, change }));
}

export function onInvalidated(
  scope: InvalidationScope,
  handler: (id: string | null, change?: RecordingChange) => void,
) {
  const listener = (event: Invalidation) => {
    if (event.scope === scope) handler(event.id, event.change);
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
      return [
        {
          scope: "recording",
          id: entity.recording_id ?? null,
          change: { liked: (action as { liked: boolean }).liked },
        },
      ];
    case "recording_title_set":
      return [
        {
          scope: "recording",
          id: entity.recording_id ?? null,
          change: {
            field: "title",
            value: (action as { title: string }).title,
          },
        },
      ];
    case "recording_artist_set":
      return [
        {
          scope: "recording",
          id: entity.recording_id ?? null,
          change: {
            field: "artist",
            value: (action as { artist: string }).artist,
          },
        },
      ];
    case "recording_title_inherit":
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
