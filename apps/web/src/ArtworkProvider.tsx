import {
  createContext,
  useCallback,
  useContext,
  useEffect,
  useMemo,
  useSyncExternalStore,
  type ReactNode,
} from "react";
import { ArtworkRetrieval, idleArtwork } from "./artworkRetrieval";

const ArtworkContext = createContext<ArtworkRetrieval | null>(null);

export function ArtworkProvider({
  account,
  children,
}: {
  account: string;
  children: ReactNode;
}) {
  const retrieval = useMemo(() => new ArtworkRetrieval(account), [account]);
  useEffect(() => {
    retrieval.open();
    return () => retrieval.close();
  }, [retrieval]);
  return (
    <ArtworkContext.Provider value={retrieval}>
      {children}
    </ArtworkContext.Provider>
  );
}

export function useRecordingArtwork(recording?: string) {
  const retrieval = useContext(ArtworkContext);
  const subscribe = useCallback(
    (listener: () => void) =>
      retrieval && recording
        ? retrieval.subscribe(recording, listener)
        : () => {},
    [retrieval, recording],
  );
  const snapshot = useCallback(
    () =>
      retrieval && recording ? retrieval.snapshot(recording) : idleArtwork,
    [retrieval, recording],
  );
  const state = useSyncExternalStore(subscribe, snapshot);
  return {
    ...state,
    missing: () => {
      if (recording) retrieval?.missing(recording);
    },
    retry: () => {
      if (recording) retrieval?.retry(recording);
    },
  };
}
