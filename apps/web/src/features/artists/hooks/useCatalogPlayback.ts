import { useEffect, useRef, useState } from "react";
import type { Schema } from "../../../shared/schema";
import { log } from "../../../shared/log";
import { errorMessage } from "../../../shared/format";
import { usePlayer } from "../../playback/components/PlaybackProvider";
import { artistEditionApi } from "../api/artistEditionApi";
import { metadataPreferencesApi } from "../../metadata/api/metadataPreferencesApi";
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

export function useCatalogPlayback(
  context: ArtistEditionContext,
  edition: string | null,
  occurrences: readonly string[],
) {
  const player = usePlayer();
  const { account, recording, association, group } = context;
  const [rows, setRows] = useState<ReadonlyMap<string, RowState>>(new Map());
  const [halted, setHalted] = useState("");
  const [consentOff, setConsentOff] = useState(false);
  const saved = useRef(new Map<string, Schema<"Recording">>());
  const running = useRef<AbortController | null>(null);

  useEffect(() => {
    saved.current = new Map();
    setRows(new Map());
    setHalted("");
    setConsentOff(false);
  }, [account, recording, association.id, group.id, edition]);

  async function search(
    occurrence: string,
    signal: AbortSignal,
  ): Promise<Resolution> {
    const known = saved.current.get(occurrence);
    if (known) return { kind: "found", recording: known };
    for (let attempt = 0; ; attempt++) {
      let outcome: Schema<"AlbumTrackAudio">;
      try {
        outcome = await artistEditionApi.audio(
          account,
          recording,
          association.id,
          {
            release_group_id: group.id,
            edition_id: edition!,
            occurrence_id: occurrence,
          },
          signal,
        );
      } catch (error) {
        if (signal.aborted) throw error;
        log.warn("catalog_track_find.failed", { reason: errorMessage(error) });
        return { kind: "failed" };
      }
      if (outcome.status === "busy" && attempt < BUSY_RETRIES) {
        await pause(BUSY_WAIT_MS, signal);
        signal.throwIfAborted();
        continue;
      }
      const result = resolution(outcome);
      if (result.kind === "found")
        saved.current.set(occurrence, result.recording);
      return result;
    }
  }

  function run(): CatalogRun {
    return {
      occurrences,
      find: search,
      mark: (occurrence, state) =>
        setRows((value) => new Map(value).set(occurrence, state)),
      halt: setHalted,
    };
  }

  async function start(from: number) {
    if (edition === null || from >= occurrences.length) return;
    running.current?.abort();
    const controller = new AbortController();
    running.current = controller;
    setHalted("");
    const select = player.prepareSelection();
    const catalog = run();
    const started = performance.now();
    let outcome = "cancelled";
    try {
      const settings = await metadataPreferencesApi.settings(
        account,
        controller.signal,
      );
      setConsentOff(!settings.automatic_service_search);
      if (!settings.automatic_service_search) {
        outcome = "consent_off";
        return;
      }
      const first = await nextFound(catalog, from, controller.signal);
      if (!first) {
        outcome = "nothing_found";
        return;
      }
      outcome = "found";
      await select({
        queue: [first.recording],
        index: 0,
        more: catalogSupply(catalog, player, first.index + 1),
      });
      outcome = "played";
    } catch (error) {
      if (!controller.signal.aborted) {
        outcome = "failed";
        setHalted(errorMessage(error));
      }
    } finally {
      log.debug("catalog_album_play.finished", {
        reason: outcome,
        duration_ms: Math.round(performance.now() - started),
      });
    }
  }

  async function shuffle() {
    if (occurrences.length === 0) return;
    await start(Math.floor(Math.random() * occurrences.length));
    if (!player.snapshot().shuffle) player.shuffle();
  }

  return { rows, halted, consentOff, start, shuffle };
}
