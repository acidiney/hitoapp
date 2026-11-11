import { createContext, useContext, useEffect } from "react";
import type { Schema } from "../../../shared/schema";
import type { Mutate } from "../../../shared/hooks/useMutation";

export type AlbumProgress = { owned: number; total: number; elsewhere: number };

export type AlbumMerge = {
  setProgress: (progress: AlbumProgress | null) => void;
  playlists: Schema<"Playlist">[];
  mutate: Mutate | null;
  setMerged: (merged: boolean) => void;
  merged: boolean;
};

export const AlbumMergeContext = createContext<AlbumMerge>({
  playlists: [],
  mutate: null,
  setProgress: () => {},
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

export function useAlbumProgress(progress: AlbumProgress | null) {
  const { setProgress } = useContext(AlbumMergeContext);
  const key = progress
    ? `${progress.owned}/${progress.total}/${progress.elsewhere}`
    : "";
  useEffect(() => {
    setProgress(progress);
    return () => setProgress(null);
  }, [key, setProgress]);
}
