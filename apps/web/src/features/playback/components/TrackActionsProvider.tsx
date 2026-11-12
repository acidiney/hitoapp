import { createContext, useContext, useMemo, type ReactNode } from "react";
import type { Schema } from "../../../shared/schema";
import { useMutation, type Mutate } from "../../../shared/hooks/useMutation";

type TrackActions = {
  account: string;
  mutate: Mutate;
  playlists: Schema<"Playlist">[];
};

const TrackActionsContext = createContext<TrackActions | null>(null);

export function TrackActionsProvider({
  account,
  playlists,
  children,
}: {
  account: string;
  playlists: Schema<"Playlist">[];
  children: ReactNode;
}) {
  const { mutate } = useMutation(account);
  const value = useMemo(
    () => ({ account, mutate, playlists }),
    [account, mutate, playlists],
  );
  return (
    <TrackActionsContext.Provider value={value}>
      {children}
    </TrackActionsContext.Provider>
  );
}

export function useTrackActions() {
  return useContext(TrackActionsContext);
}
