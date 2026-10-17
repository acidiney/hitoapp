import { useEffect, useMemo, useRef, useState } from "react";
import { HttpError } from "../http";
import { log } from "../log";
import { podcastApi } from "../podcastApi";
import type { PodcastEpisodePage, RecordingProgress } from "../types";
import { useRequestScope } from "./useRequestScope";

type Target = {
  cursor: string | null;
  offset: number;
  minimumRevision?: number;
};
type Owner = {
  page: PodcastEpisodePage;
  progress: RecordingProgress[];
  renderedPage: PodcastEpisodePage;
  renderedProgress: RecordingProgress[];
  highestRevision: number;
  generation: object;
  offset: number;
  run: AbortController | null;
  target: Target | null;
  stale: boolean;
  refreshing: boolean;
  error: string;
};
type State = {
  owner: Owner;
  page: PodcastEpisodePage;
  progress: RecordingProgress[];
  offset: number;
  loading: boolean;
  stale: boolean;
  refreshing: boolean;
  error: string;
};

export function usePodcastEpisodes(
  account: string,
  subscription: string,
  renderedPage: PodcastEpisodePage,
  renderedProgress: RecordingProgress[],
) {
  const scope = useRequestScope(JSON.stringify([account, subscription]));
  const owner = useMemo<Owner>(
    () => ({
      page: renderedPage,
      progress: renderedProgress,
      renderedPage,
      renderedProgress,
      highestRevision: renderedPage.content_revision,
      generation: {},
      offset: 0,
      run: null,
      target: null,
      stale: false,
      refreshing: false,
      error: "",
    }),
    [account, subscription],
  );
  const latest = useRef(owner);
  latest.current = owner;
  const [state, setState] = useState<State | null>(null);
  const current = state?.owner === owner ? state : null;
  const incoming =
    renderedPage.content_revision >= owner.highestRevision &&
    (owner.renderedPage !== renderedPage ||
      owner.renderedProgress !== renderedProgress);

  function publish() {
    setState({
      owner,
      page: owner.page,
      progress: owner.progress,
      offset: owner.offset,
      loading: owner.run !== null,
      stale: owner.stale,
      refreshing: owner.refreshing,
      error: owner.error,
    });
  }

  async function load(target: Target): Promise<void> {
    const request = scope.capture();
    if (
      !request.active() ||
      owner.run ||
      owner.refreshing ||
      latest.current !== owner
    )
      return;
    const run = new AbortController();
    const signal = AbortSignal.any([request.signal, run.signal]);
    const active = () =>
      request.active() &&
      !signal.aborted &&
      latest.current === owner &&
      owner.run === run;
    const started = performance.now();
    owner.run = run;
    owner.target = target;
    owner.error = "";
    publish();
    log.info("podcasts.episodes_requested", {
      account_id: account,
      index: target.offset,
    });
    try {
      const page = await podcastApi.episodes(
        account,
        subscription,
        Math.min(200, Math.max(1, owner.page.limit)),
        target.cursor,
        signal,
      );
      if (!active()) return;
      if (
        page.content_revision <
        Math.max(target.minimumRevision ?? 0, owner.highestRevision)
      )
        throw new Error("Hito returned an older episode page.");
      if (page.items.length > 200)
        throw new Error("Hito returned too many episodes.");
      owner.highestRevision = Math.max(
        owner.highestRevision,
        page.content_revision,
      );
      const ids = [
        ...new Set(page.items.map((episode) => episode.recording.id)),
      ];
      const progress = ids.length
        ? await podcastApi.progress(account, ids, signal)
        : [];
      if (!active()) return;
      owner.page = page;
      owner.progress = progress;
      owner.offset = target.offset;
      owner.target = null;
      owner.stale = false;
      log.info("podcasts.episodes_loaded", {
        account_id: account,
        content_revision: page.content_revision,
        count: page.items.length,
        index: target.offset,
        duration_ms: Math.round(performance.now() - started),
      });
    } catch (error) {
      if (active()) {
        const stale = error instanceof HttpError && error.status === 409;
        if (stale) {
          owner.stale = true;
          owner.target = { cursor: null, offset: 0 };
        }
        owner.error = stale
          ? "This feed changed. Load the latest episodes to continue browsing."
          : "Hito could not load these episodes and their saved positions. The previously shown episodes are still available.";
        log.warn("podcasts.episodes_failed", {
          account_id: account,
          reason: error instanceof HttpError ? "http" : "request",
          status: error instanceof HttpError ? error.status : undefined,
          duration_ms: Math.round(performance.now() - started),
        });
      }
    } finally {
      if (owner.run === run) {
        owner.run = null;
        if (request.active() && !signal.aborted && latest.current === owner)
          publish();
      }
      if (!request.active() || signal.aborted || latest.current !== owner)
        log.debug("podcasts.episodes_cancelled", { account_id: account });
    }
  }

  useEffect(() => () => owner.run?.abort(), [owner]);

  useEffect(() => {
    if (
      renderedPage.content_revision < owner.highestRevision ||
      (owner.renderedPage === renderedPage &&
        owner.renderedProgress === renderedProgress)
    )
      return;
    owner.run?.abort();
    owner.run = null;
    owner.renderedPage = renderedPage;
    owner.renderedProgress = renderedProgress;
    owner.highestRevision = renderedPage.content_revision;
    owner.generation = {};
    owner.page = renderedPage;
    owner.progress = renderedProgress;
    owner.offset = 0;
    owner.target = null;
    owner.stale = false;
    owner.error = "";
    publish();
  }, [owner, renderedPage, renderedProgress]);

  return {
    page: incoming ? renderedPage : (current?.page ?? owner.page),
    progress: incoming
      ? renderedProgress
      : (current?.progress ?? owner.progress),
    offset: incoming ? 0 : (current?.offset ?? 0),
    loading: !incoming && (current?.loading ?? false),
    stale: !incoming && (current?.stale ?? false),
    refreshing: current?.refreshing ?? false,
    generation: owner.generation,
    knownRevision: owner.highestRevision,
    error: incoming ? "" : (current?.error ?? ""),
    next: () =>
      owner.page.next_cursor && !owner.stale && !owner.refreshing
        ? load({
            cursor: owner.page.next_cursor,
            offset: owner.offset + owner.page.items.length,
          })
        : Promise.resolve(),
    first: () =>
      load({
        cursor: null,
        offset: 0,
        minimumRevision: owner.target?.minimumRevision,
      }),
    retry: () => (owner.target ? load(owner.target) : Promise.resolve()),
    pause: () => {
      if (!scope.capture().active() || latest.current !== owner) return;
      owner.run?.abort();
      owner.run = null;
      owner.refreshing = true;
      publish();
    },
    applyRevision: (revision: number) => {
      if (!scope.capture().active() || latest.current !== owner)
        return Promise.resolve();
      owner.refreshing = false;
      owner.highestRevision = Math.max(owner.highestRevision, revision);
      if (owner.highestRevision === owner.page.content_revision) {
        publish();
        return Promise.resolve();
      }
      if (
        owner.run &&
        (owner.target?.minimumRevision ?? 0) >= owner.highestRevision
      )
        return Promise.resolve();
      owner.run?.abort();
      owner.run = null;
      owner.stale = true;
      return load({
        cursor: null,
        offset: 0,
        minimumRevision: owner.highestRevision,
      });
    },
  };
}
