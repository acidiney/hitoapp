import { useEffect, useRef, useState } from "react";
import type { Schema } from "../../../shared/schema";
import type { LyricFailure } from "../model/lyricFailure";
import {
  alreadySaved,
  identityKey,
  lyricIdentityLimit,
  type LyricIdentity,
} from "../model/lyricIdentity";
import { previewLines } from "../model/lyricContent";
import { lyricQuerySeed, useLyricQuery } from "./useLyricQuery";
import { useLyricIdentities } from "./useLyricIdentities";
import { useLyricRemoval } from "./useLyricRemoval";
import { useLyricReview } from "./useLyricReview";
import { useLyricSearch } from "./useLyricSearch";
import { useSelectedLyrics } from "./useSelectedLyrics";
import { useAutoLyricIdentity } from "./useAutoLyricIdentity";

type Chosen = { scope: string; id: string };
type Notice = { scope: string; text: string };

export function useRecordingLyrics(
  account: string,
  detail: Schema<"RecordingMetadata">,
  automatic: boolean,
  active: boolean,
) {
  const recording = detail.recording_id;
  const scopeKey = JSON.stringify([account, recording]);
  const [lostScope, setLostScope] = useState<string | null>(null);
  const [chosenService, setChosenService] = useState<Chosen | null>(null);
  const [notice, setNotice] = useState<Notice | null>(null);
  const resets = useRef<() => void>(() => {});
  const reload = useRef<() => void>(() => {});

  const lose = () => {
    setLostScope(scopeKey);
    resets.current();
  };
  const onFailure = (failure: LyricFailure) => {
    if (failure.kind === "access") lose();
    else if (failure.reload) reload.current();
  };

  const identities = useLyricIdentities(account, recording, lose);
  const read = useSelectedLyrics(
    account,
    recording,
    identities.selected,
    onFailure,
  );
  const removal = useLyricRemoval(account, recording, onFailure);
  const auto = useAutoLyricIdentity(
    account,
    recording,
    {
      title: detail.title.value,
      artist: detail.artist.value,
      durationMs: detail.selected_observation?.duration_ms ?? null,
    },
    onFailure,
  );
  const services = identities.services ?? [];
  const service =
    services.find(
      (item) =>
        chosenService?.scope === scopeKey &&
        item.installation_id === chosenService.id,
    ) ??
    services[0] ??
    null;
  const search = useLyricSearch(account, recording, service, onFailure);
  const review = useLyricReview(account, recording, service, onFailure);
  const queryState = useLyricQuery(
    scopeKey,
    lyricQuerySeed(detail.title.value, detail.artist.value),
  );

  resets.current = () => {
    identities.reset();
    read.cancel();
    auto.cancel();
    search.cancel();
    review.clear();
    removal.clear();
  };
  reload.current = () => void identities.load();

  const lost = lostScope === scopeKey;
  const wanted = (active || automatic) && !lost;
  const loader = useRef(identities.load);
  loader.current = identities.load;
  useEffect(() => {
    if (wanted && !identities.loaded && !identities.loading)
      void loader.current();
  }, [wanted, identities.loaded, identities.loading, scopeKey]);

  const attempted = useRef<string | null>(null);
  const reader = useRef(read.load);
  reader.current = read.load;
  const selectedKey = identities.selected && identityKey(identities.selected);
  const autoReadKey = JSON.stringify([scopeKey, selectedKey]);
  useEffect(() => {
    if (
      !automatic ||
      lost ||
      !selectedKey ||
      identities.origin !== "default" ||
      attempted.current === autoReadKey
    )
      return;
    attempted.current = autoReadKey;
    void reader.current();
  }, [automatic, lost, selectedKey, identities.origin, autoReadKey]);

  const autoLoader = useRef(auto.lookup);
  autoLoader.current = auto.lookup;
  const identitiesLoader = useRef(identities.load);
  identitiesLoader.current = identities.load;
  const lookupAttempted = useRef<string | null>(null);
  const noSavedIdentity =
    identities.loaded &&
    !identities.failure &&
    identities.identities !== null &&
    identities.identities.length === 0;
  const startLookup = () =>
    void autoLoader.current((identity) =>
      identitiesLoader.current({ prefer: identityKey(identity) }),
    );
  useEffect(() => {
    if (
      !automatic ||
      lost ||
      !noSavedIdentity ||
      auto.noMatch ||
      lookupAttempted.current === scopeKey
    )
      return;
    lookupAttempted.current = scopeKey;
    startLookup();
  }, [automatic, lost, noSavedIdentity, auto.noMatch, scopeKey]);

  function accepted(identity: LyricIdentity) {
    setNotice({
      scope: scopeKey,
      text: `Saved ${identity.label}. Choose Show lyrics to read it.`,
    });
    void identities.load({ prefer: identityKey(identity), byUser: true });
  }

  function removeIdentity(identity: LyricIdentity) {
    if (
      identities.selected &&
      identityKey(identities.selected) === identityKey(identity)
    )
      read.cancel();
    setNotice(null);
    void removal.remove(identity, () => void identities.load({ byUser: true }));
  }

  function retryAfterAccessLoss() {
    setLostScope(null);
  }

  const items = identities.identities ?? [];
  const atLimit = items.length >= lyricIdentityLimit;
  const outcome = read.outcome;
  return {
    lost,
    retryAfterAccessLoss,
    identities: {
      loading: identities.loading,
      loaded: identities.loaded,
      items: identities.identities,
      failure: identities.failure,
      selected: identities.selected,
      select: identities.select,
      reload: () => void identities.load(),
      atLimit,
      removing: removal.removing,
      removalFailure: removal.failure,
      remove: removeIdentity,
    },
    reader: {
      pending: read.pending,
      outcome,
      failure: read.failure,
      load: () => void read.load(),
      cancel: read.cancel,
    },
    lookup: {
      engaged:
        noSavedIdentity &&
        (auto.pending ||
          auto.noMatch ||
          auto.failure !== null ||
          identities.loading),
      pending: auto.pending || identities.loading,
      failure: auto.failure,
      run: startLookup,
    },
    finder: {
      services: identities.services,
      servicesFailure: identities.servicesFailure,
      service,
      chooseService: (id: string) => {
        search.cancel();
        review.clear();
        setChosenService({ scope: scopeKey, id });
      },
      query: queryState.query,
      setQuery: queryState.setQuery,
      search,
      review,
      accept: () => void review.accept(accepted),
      duplicate:
        review.candidate && service
          ? alreadySaved(items, service.installation_id, review.candidate.id)
          : false,
      notice: notice?.scope === scopeKey ? notice.text : null,
    },
    preview: outcome?.kind === "lyrics" ? previewLines(outcome.view) : [],
  };
}
