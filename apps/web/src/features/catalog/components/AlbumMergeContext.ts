import { createContext, useContext, useEffect, useRef } from "react";
import type { Schema } from "../../../shared/schema";
import type { Mutate } from "../../../shared/hooks/useMutation";

export type AlbumProgress = { owned: number; total: number; elsewhere: number };

export type EditionPlayState = {
  tracks: number;
  durationMs: number | null;
  consentOff: boolean;
  halted: string;
};

export type EditionPlay = EditionPlayState & {
  play: () => Promise<boolean>;
  shuffle: () => Promise<void>;
};

export type AlbumMerge = {
  editionPlay: EditionPlay | null;
  setEditionPlay: (play: EditionPlay | null) => void;
  setProgress: (progress: AlbumProgress | null) => void;
  playlists: Schema<"Playlist">[];
  mutate: Mutate | null;
  setMerged: (merged: boolean) => void;
  merged: boolean;
  setEditionLoading: (loading: boolean) => void;
};

export const AlbumMergeContext = createContext<AlbumMerge>({
  editionPlay: null,
  setEditionPlay: () => {},
  playlists: [],
  mutate: null,
  setProgress: () => {},
  setMerged: () => {},
  merged: false,
  setEditionLoading: () => {},
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
  const owned = progress?.owned;
  const total = progress?.total;
  const elsewhere = progress?.elsewhere;
  useEffect(() => {
    setProgress(
      owned === undefined || total === undefined || elsewhere === undefined
        ? null
        : { owned, total, elsewhere },
    );
    return () => setProgress(null);
  }, [owned, total, elsewhere, setProgress]);
}

export function useEditionPlayPublish(
  state: EditionPlayState | null,
  actions: Pick<EditionPlay, "play" | "shuffle">,
) {
  const { setEditionPlay } = useContext(AlbumMergeContext);
  const latest = useRef(actions);
  latest.current = actions;
  const tracks = state?.tracks;
  const durationMs = state?.durationMs;
  const consentOff = state?.consentOff;
  const halted = state?.halted;
  useEffect(() => {
    setEditionPlay(
      tracks === undefined ||
        durationMs === undefined ||
        consentOff === undefined ||
        halted === undefined
        ? null
        : {
            tracks,
            durationMs,
            consentOff,
            halted,
            play: () => latest.current.play(),
            shuffle: () => latest.current.shuffle(),
          },
    );
    return () => setEditionPlay(null);
  }, [tracks, durationMs, consentOff, halted, setEditionPlay]);
}

export function useEditionLoading(loading: boolean) {
  const { setEditionLoading } = useContext(AlbumMergeContext);
  useEffect(() => {
    setEditionLoading(loading);
  }, [loading, setEditionLoading]);
  useEffect(() => () => setEditionLoading(false), [setEditionLoading]);
}
