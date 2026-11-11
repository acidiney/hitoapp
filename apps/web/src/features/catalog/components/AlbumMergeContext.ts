import { createContext, useContext, useEffect } from "react";
import type { Schema } from "../../../shared/schema";
import type { Mutate } from "../../../shared/hooks/useMutation";

export type AlbumMerge = {
  playlists: Schema<"Playlist">[];
  mutate: Mutate | null;
  setMerged: (merged: boolean) => void;
  merged: boolean;
};

export const AlbumMergeContext = createContext<AlbumMerge>({
  playlists: [],
  mutate: null,
  setMerged: () => {},
  merged: false,
});

export function useAlbumMerge(active: boolean): AlbumMerge {
  const merge = useContext(AlbumMergeContext);
  const { setMerged } = merge;
  useEffect(() => {
    if (!active) return;
    setMerged(true);
    return () => setMerged(false);
  }, [active, setMerged]);
  return merge;
}
