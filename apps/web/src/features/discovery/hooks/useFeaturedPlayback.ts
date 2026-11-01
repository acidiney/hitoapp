import type { Schema } from "../../../shared/schema";
import { router } from "@inertiajs/react";
import { useRef, useState } from "react";
import { errorMessage } from "../../../shared/format";
import { log } from "../../../shared/log";
import { useRequestScope } from "../../../shared/hooks/useRequestScope";
import { createUuid } from "../../../shared/uuid";
import { pluginsApi } from "../../plugins/api/pluginsApi";
import { playlistDetailApi } from "../../playlists/api/playlistDetailApi";
import type { Player, Track } from "../../playback/model";

const REQUEST_DEADLINE_MS = 45_000;

export const featuredKey = (entry: Schema<"FeaturedEntry">) =>
  `${entry.installation_id}:${entry.id}`;

export type FeaturedPlaybackState = {
  pending: string | null;
  notice: { tone: "status" | "error"; text: string } | null;
};

function queueOf(entries: Schema<"PlaylistEntry">[]): Track[] {
  return entries.flatMap((entry) =>
    entry.recording_id == null
      ? []
      : [
          {
            id: entry.recording_id,
            title: entry.title,
            artist: entry.artist,
            inherited_title: entry.inherited_title,
            inherited_artist: entry.inherited_artist,
            duration_ms: entry.duration_ms,
            available: entry.available,
            plays_from_services: entry.plays_from_services,
          } as Track,
        ],
  );
}

export function useFeaturedPlayback(
  account: string,
  player: Pick<Player, "playQueue">,
) {
  const scope = useRequestScope(account);
  const busy = useRef(false);
  const [state, setState] = useState<FeaturedPlaybackState>({
    pending: null,
    notice: null,
  });

  async function run(entry: Schema<"FeaturedEntry">, action: "play" | "open") {
    const request = scope.capture();
    if (busy.current || !request.active()) return;
    busy.current = true;
    const requestId = createUuid();
    const started = performance.now();
    const controller = new AbortController();
    const cancel = () => controller.abort(request.signal.reason);
    request.signal.addEventListener("abort", cancel, { once: true });
    const timer = setTimeout(
      () => controller.abort(new Error("This took too long. Try again.")),
      REQUEST_DEADLINE_MS,
    );
    let outcome = "cancelled";
    setState({ pending: featuredKey(entry), notice: null });
    log.debug("featured_card.started", {
      request_id: requestId,
      action,
    });
    try {
      const receipt = await pluginsApi.importPlaylist(
        account,
        entry.installation_id,
        {
          request_id: requestId,
          installation_revision: entry.installation_revision,
          playlist_reference: entry.reference,
        },
        controller.signal,
      );
      if (!request.active()) return;
      if (receipt.request_id !== requestId)
        throw new Error("Hito returned a different import receipt. Try again.");
      if (action === "open") {
        outcome = "opened";
        setState({ pending: null, notice: null });
        router.visit(`/playlists/${encodeURIComponent(receipt.playlist_id)}`);
        return;
      }
      const detail = await playlistDetailApi.read(
        account,
        receipt.playlist_id,
        controller.signal,
      );
      if (!request.active()) return;
      const queue = queueOf(detail.entries);
      if (queue.length === 0) {
        outcome = "empty";
        setState({
          pending: null,
          notice: {
            tone: "error",
            text: `Nothing playable from ${entry.title} yet. A copy is in your Playlists.`,
          },
        });
        return;
      }
      await player.playQueue(queue);
      if (!request.active()) return;
      outcome = "playing";
      setState({
        pending: null,
        notice: {
          tone: "status",
          text: `Playing ${entry.title}. A copy was added to your Playlists.`,
        },
      });
    } catch (error) {
      if (request.active()) {
        outcome = "failed";
        setState({
          pending: null,
          notice: { tone: "error", text: errorMessage(error) },
        });
      }
    } finally {
      busy.current = false;
      clearTimeout(timer);
      request.signal.removeEventListener("abort", cancel);
      const record = outcome === "failed" ? log.warn : log.debug;
      record("featured_card.finished", {
        request_id: requestId,
        action,
        reason: outcome,
        duration_ms: Math.round(performance.now() - started),
      });
    }
  }

  return {
    ...state,
    play: (entry: Schema<"FeaturedEntry">) => run(entry, "play"),
    open: (entry: Schema<"FeaturedEntry">) => run(entry, "open"),
  };
}
