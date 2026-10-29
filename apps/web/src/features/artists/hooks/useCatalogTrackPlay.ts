import { useMemo, useState } from "react";
import { pluginsApi } from "../../plugins/api/pluginsApi";
import { albumEditionApi } from "../api/albumEditionApi";
import { errorMessage } from "../../../shared/format";
import { log } from "../../../shared/log";
import type {
  AlbumTrackAudio,
  CatalogAdoption,
  FindAlbumTrackAudio,
  Recording,
} from "../../../shared/types";
import { createUuid } from "../../../shared/uuid";
import { useRequestScope } from "../../../shared/hooks/useRequestScope";
import type { Player } from "../../playback/model";

type Owner = { pending: string | null; saved: ReadonlyMap<string, Recording> };
type State = {
  owner: Owner;
  saved: ReadonlyMap<string, Recording>;
  pending: string | null;
  failed: { id: string; message: string } | null;
};
type Obtain = (signal: AbortSignal) => Promise<Recording>;

export type AudioSearch = { release: string; request: FindAlbumTrackAudio };

const searchMessages: Record<
  Exclude<AlbumTrackAudio["status"], "found">,
  string
> = {
  no_match:
    "No approved provider has a confident match for this track. Nothing was saved.",
  no_providers:
    "Connect an audio provider in Settings to play tracks that are not in your library.",
  disabled: "Metadata providers are disabled in your settings.",
  unsupported: "This provider does not support complete edition track lists.",
  unavailable:
    "Audio providers are unavailable right now. Nothing was saved. Try again.",
  cancelled: "The audio search was cancelled.",
  needs_choice: "Choose the edition again to find audio for this track.",
};

export const foundKey = (occurrence: string) => `occurrence:${occurrence}`;

export function useCatalogTrackPlay(
  account: string,
  adoption: CatalogAdoption | null,
  player: Pick<Player, "prepareSelection">,
) {
  const scope = useRequestScope(JSON.stringify([account, adoption]));
  const owner = useMemo<Owner>(
    () => ({ pending: null, saved: new Map() }),
    [scope],
  );
  const [snapshot, setSnapshot] = useState<State | null>(null);
  const current = snapshot?.owner === owner ? snapshot : null;

  async function run(
    occurrence: string,
    key: string,
    action: string,
    obtain: Obtain,
  ) {
    const request = scope.capture();
    if (!request.active() || owner.pending !== null) return;
    owner.pending = occurrence;
    const requestId = createUuid();
    const started = performance.now();
    let outcome = "cancelled";
    const update = (change: Partial<State>) => {
      if (request.active())
        setSnapshot({
          owner,
          saved: owner.saved,
          pending: owner.pending,
          failed: null,
          ...change,
        });
    };
    update({ pending: occurrence });
    log.debug(`${action}.started`, { request_id: requestId });
    try {
      const prepared = player.prepareSelection();
      let recording = owner.saved.get(key);
      if (!recording) {
        recording = await obtain(request.signal);
        if (!request.active()) return;
        owner.saved = new Map(owner.saved).set(key, recording);
      }
      outcome = "saved";
      await prepared(recording);
      if (request.active()) outcome = "played";
    } catch (error) {
      if (request.active()) {
        outcome = "failed";
        update({
          pending: null,
          failed: { id: occurrence, message: errorMessage(error) },
        });
      }
    } finally {
      owner.pending = null;
      if (outcome !== "failed") update({ pending: null });
      const record = outcome === "failed" ? log.warn : log.debug;
      record(`${action}.finished`, {
        request_id: requestId,
        reason: outcome,
        duration_ms: Math.round(performance.now() - started),
      });
    }
  }

  function adopt(
    installation: string,
    revision: number,
    providerRecording: string,
  ): Obtain {
    return (signal) =>
      pluginsApi.adopt(
        account,
        installation,
        {
          installation_revision: revision,
          provider_recording_id: providerRecording,
        },
        signal,
      );
  }

  async function play(occurrence: string, providerRecording: string) {
    if (!adoption) return;
    await run(
      occurrence,
      providerRecording,
      "catalog_track_play",
      adopt(
        adoption.installation_id,
        adoption.installation_revision,
        providerRecording,
      ),
    );
  }

  async function findAndPlay(occurrence: string, search: AudioSearch) {
    await run(
      occurrence,
      foundKey(occurrence),
      "catalog_track_find",
      async (signal) => {
        const found = await albumEditionApi.audio(
          account,
          search.release,
          search.request,
          signal,
        );
        signal.throwIfAborted();
        if (found.status !== "found")
          throw new Error(searchMessages[found.status]);
        return adopt(
          found.installation_id,
          found.installation_revision,
          found.provider_recording_id,
        )(signal);
      },
    );
  }

  return {
    saved: current?.saved ?? owner.saved,
    pending: current?.pending ?? null,
    failed: current?.failed ?? null,
    play,
    findAndPlay,
  };
}
