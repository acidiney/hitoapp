import { useMemo, useState } from "react";
import { pluginsApi } from "../../plugins/api/pluginsApi";
import { errorMessage } from "../../../shared/format";
import { log } from "../../../shared/log";
import type { CatalogAdoption, Recording } from "../../../shared/types";
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

  async function play(occurrence: string, providerRecording: string) {
    const request = scope.capture();
    if (!request.active() || !adoption || owner.pending !== null) return;
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
    log.debug("catalog_track_play.started", { request_id: requestId });
    try {
      const prepared = player.prepareSelection();
      let recording = owner.saved.get(providerRecording);
      if (!recording) {
        recording = await pluginsApi.adopt(
          account,
          adoption.installation_id,
          {
            installation_revision: adoption.installation_revision,
            provider_recording_id: providerRecording,
          },
          request.signal,
        );
        if (!request.active()) return;
        owner.saved = new Map(owner.saved).set(providerRecording, recording);
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
      record("catalog_track_play.finished", {
        request_id: requestId,
        reason: outcome,
        duration_ms: Math.round(performance.now() - started),
      });
    }
  }

  return {
    saved: current?.saved ?? owner.saved,
    pending: current?.pending ?? null,
    failed: current?.failed ?? null,
    play,
  };
}
