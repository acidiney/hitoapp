import {
  createContext,
  useCallback,
  useContext,
  useEffect,
  useMemo,
  useSyncExternalStore,
  type ReactNode,
} from "react";
import {
  ArtworkRetrieval,
  idleArtwork,
  type ArtworkKind,
} from "../api/artworkRetrieval";
import { onInvalidated } from "../../../shared/invalidation";

function artworkTarget(id: string) {
  const [kind, ...rest] = id.split(":");
  return kind === "recording" || kind === "release" || kind === "playlist"
    ? { kind: kind as ArtworkKind, id: rest.join(":") }
    : undefined;
}

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
  useEffect(
    () =>
      onInvalidated("artwork", (changed) => {
        const target = changed && artworkTarget(changed);
        if (target) retrieval.refresh(target);
      }),
    [retrieval],
  );
  return (
    <ArtworkContext.Provider value={retrieval}>
      {children}
    </ArtworkContext.Provider>
  );
}

export function useRefreshArtworkAvailability(account: string) {
  const retrieval = useContext(ArtworkContext);
  if (!retrieval || retrieval.account !== account)
    throw new Error("Plugin settings require their account artwork provider");
  return useCallback(() => retrieval.refreshAvailability(), [retrieval]);
}

export function useArtwork(kind: ArtworkKind, id?: string) {
  const retrieval = useContext(ArtworkContext);
  const target = useMemo(() => (id ? { kind, id } : undefined), [kind, id]);
  const subscribe = useCallback(
    (listener: () => void) =>
      retrieval && target ? retrieval.subscribe(target, listener) : () => {},
    [retrieval, target],
  );
  const snapshot = useCallback(
    () => (retrieval && target ? retrieval.snapshot(target) : idleArtwork),
    [retrieval, target],
  );
  const state = useSyncExternalStore(subscribe, snapshot);
  return {
    ...state,
    generation: retrieval?.generation,
    missing: () => {
      if (target) retrieval?.missing(target, state.revision);
    },
    loaded: () => {
      if (target) retrieval?.loaded(target, state.revision);
    },
    retry: () => {
      if (target) retrieval?.retry(target);
    },
  };
}
