import { useEffect, useRef, useState } from "react";
import type { Schema } from "../../../shared/schema";
import { log } from "../../../shared/log";
import { errorMessage } from "../../../shared/format";
import { usePlayer } from "../../playback/components/PlaybackProvider";
import { artistEditionApi } from "../api/artistEditionApi";
import {
  catalogSupply,
  nextFound,
  resolution,
  type CatalogRun,
  type Resolution,
  type RowState,
} from "../model/catalogQueue";
import type { ArtistEditionContext } from "./useArtistEditions";

const BUSY_RETRIES = 2;
const BUSY_WAIT_MS = 700;

function pause(ms: number, signal: AbortSignal) {
  return new Promise<void>((resolve) => {
    const timer = setTimeout(resolve, ms);
    signal.addEventListener(
      "abort",
      () => {
        clearTimeout(timer);
        resolve();
      },
      { once: true },
    );
  });
}

async function whileEither<T>(
  first: AbortSignal,
  second: AbortSignal,
  work: (signal: AbortSignal) => Promise<T>,
) {
  const joined = new AbortController();
  const stop = () => joined.abort();
  first.addEventListener("abort", stop, { once: true });
  second.addEventListener("abort", stop, { once: true });
  if (first.aborted || second.aborted) stop();
  try {
    return await work(joined.signal);
  } finally {
    first.removeEventListener("abort", stop);
    second.removeEventListener("abort", stop);
  }
}

export type CatalogAudio = {
  scope: string;
  find(
    edition: string,
    occurrence: string,
    signal: AbortSignal,
  ): Promise<Schema<"AlbumTrackAudio">>;
};

export function useCatalogPlayback(
  context: ArtistEditionContext,
  edition: string | null,
  occurrences: readonly string[],
  remembered: ReadonlyMap<string, Schema<"Recording">>,
) {
  const { account, recording, association, group } = context;
  return useEditionPlayback(
    {
      scope: JSON.stringify([account, recording, association.id, group.id]),
      find: (edition, occurrence, signal) =>
        artistEditionApi.audio(
          account,
          recording,
          association.id,
          {
            release_group_id: group.id,
            edition_id: edition,
            occurrence_id: occurrence,
          },
          signal,
        ),
    },
    edition,
    occurrences,
    remembered,
  );
}

export function useEditionPlayback(
  audio: CatalogAudio,
  edition: string | null,
  occurrences: readonly string[],
  remembered: ReadonlyMap<string, Schema<"Recording">>,
) {
  const player = usePlayer();
  const [rows, setRows] = useState<ReadonlyMap<string, RowState>>(new Map());
  const [recordings, setRecordings] = useState<
    ReadonlyMap<string, Schema<"Recording">>
  >(new Map());
  const finders = useRef(new Set<AbortController>());
  const [halted, setHalted] = useState("");
  const [consentOff, setConsentOff] = useState(false);
  const saved = useRef(new Map<string, Schema<"Recording">>());
  const missed = useRef(new Map<string, "not_found" | "failed">());
  const running = useRef<AbortController | null>(null);
  const [life] = useState(() => {
    const state = {
      epoch: 0,
      retire: () => {
        state.epoch++;
      },
    };
    return state;
  });

  useEffect(() => {
    const pending = finders.current;
    saved.current = new Map();
    missed.current = new Map();
    setRows(new Map());
    setRecordings(new Map());
    setHalted("");
    setConsentOff(false);
    return () => {
      life.retire();
      running.current?.abort();
      running.current = null;
      for (const finder of pending) finder.abort();
      pending.clear();
    };
  }, [life, audio.scope, edition]);

  useEffect(() => {
    for (const [occurrence, recording] of remembered)
      if (!saved.current.has(occurrence))
        saved.current.set(occurrence, recording);
    setRecordings(new Map(saved.current));
  }, [remembered]);

  async function search(
    occurrence: string,
    signal: AbortSignal,
  ): Promise<Resolution> {
    const known = saved.current.get(occurrence);
    if (known) return { kind: "found", recording: known };
    const miss = missed.current.get(occurrence);
    if (miss) return { kind: miss };
    for (let attempt = 0; ; attempt++) {
      let outcome: Schema<"AlbumTrackAudio">;
      try {
        outcome = await audio.find(edition!, occurrence, signal);
      } catch (error) {
        if (signal.aborted) throw error;
        log.warn("catalog_track_find.failed", { reason: errorMessage(error) });
        missed.current.set(occurrence, "failed");
        return { kind: "failed" };
      }
      if (outcome.status === "busy" && attempt < BUSY_RETRIES) {
        await pause(BUSY_WAIT_MS, signal);
        signal.throwIfAborted();
        continue;
      }
      const result = resolution(outcome);
      if (result.kind === "not_found" || result.kind === "failed")
        missed.current.set(occurrence, result.kind);
      if (result.kind === "found") {
        saved.current.set(occurrence, result.recording);
        if (!signal.aborted) setRecordings(new Map(saved.current));
      }
      return result;
    }
  }

  function run(controller: AbortController, owned: boolean): CatalogRun {
    const mine = life.epoch;
    return {
      occurrences,
      failures: 0,
      ownedOnly: false,
      known: (occurrence) => saved.current.get(occurrence),
      stopped: () => owned && controller.signal.aborted,
      find: (occurrence, signal) =>
        owned
          ? whileEither(signal, controller.signal, (joined) =>
              search(occurrence, joined),
            )
          : search(occurrence, signal),
      mark: (occurrence, state) => {
        if (life.epoch === mine)
          setRows((value) => new Map(value).set(occurrence, state));
      },
      halt: (stop) => {
        if (life.epoch !== mine) return;
        if (stop.consent) setConsentOff(true);
        else setHalted(stop.message);
      },
    };
  }

  function seed(known: ReadonlyMap<string, Schema<"Recording">>) {
    for (const [occurrence, recording] of known)
      if (!saved.current.has(occurrence))
        saved.current.set(occurrence, recording);
    setRecordings(new Map(saved.current));
  }

  async function find(index: number): Promise<Schema<"Recording"> | null> {
    const occurrence = occurrences[index];
    if (edition === null || occurrence === undefined) return null;
    const known = saved.current.get(occurrence);
    if (known) return known;
    missed.current.delete(occurrence);
    const controller = new AbortController();
    finders.current.add(controller);
    const catalog = run(controller, true);
    catalog.mark(occurrence, { status: "finding" });
    try {
      const result = await catalog.find(occurrence, controller.signal);
      if (result.kind === "found") {
        catalog.mark(occurrence, {
          status: "found",
          recording: result.recording.id,
        });
        return result.recording;
      } else if (result.kind === "stop") {
        catalog.mark(occurrence, { status: "idle" });
        catalog.halt(result);
      } else catalog.mark(occurrence, { status: result.kind });
    } catch {
      catalog.mark(occurrence, { status: "idle" });
    } finally {
      finders.current.delete(controller);
    }
    return null;
  }

  async function start(from: number): Promise<boolean> {
    if (edition === null || from >= occurrences.length) return false;
    running.current?.abort();
    missed.current.delete(occurrences[from]);
    const controller = new AbortController();
    running.current = controller;
    setHalted("");
    setConsentOff(false);
    const select = player.prepareSelection();
    const catalog = run(controller, true);
    const started = performance.now();
    let outcome = "cancelled";
    try {
      const first = await nextFound(catalog, from, controller.signal);
      if (!first) {
        outcome = "nothing_found";
        return false;
      }
      if (controller.signal.aborted) return false;
      outcome = "found";
      const feeding = run(controller, false);
      feeding.failures = catalog.failures;
      feeding.ownedOnly = catalog.ownedOnly;
      const began = await select({
        queue: [first.recording],
        index: 0,
        more: catalogSupply(feeding, player, first.index + 1),
      });
      outcome = began ? "played" : "superseded";
      return began;
    } catch (error) {
      if (!controller.signal.aborted) {
        outcome = "failed";
        setHalted(errorMessage(error));
      }
      return false;
    } finally {
      log.debug("catalog_album_play.finished", {
        reason: outcome,
        duration_ms: Math.round(performance.now() - started),
      });
    }
  }

  async function shuffle() {
    if (occurrences.length === 0) return;
    const began = await start(Math.floor(Math.random() * occurrences.length));
    if (began && !player.snapshot().shuffle) player.shuffle();
  }

  return { rows, recordings, halted, consentOff, start, shuffle, find, seed };
}
