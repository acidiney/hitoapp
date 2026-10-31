import type { Schema } from "../../../shared/schema";
import { useState } from "react";
import { errorMessage } from "../../../shared/format";
import { log } from "../../../shared/log";
import { useRequestScope } from "../../../shared/hooks/useRequestScope";
import { createUuid } from "../../../shared/uuid";
import type { Player } from "../../playback/model";
import { radioApi } from "../api/radioApi";

const REQUEST_DEADLINE_MS = 25_000;

export type RadioState = {
  /** The chosen artist whose radio is being built. */
  pending: string | null;
  notice: { tone: "status" | "error"; text: string } | null;
};

function mixText(radio: Schema<"ArtistRadio">): string {
  if (radio.similar_artists === 0)
    return `Playing a mix of ${radio.artist} from your library.`;
  const others = `${radio.similar_artists} similar ${radio.similar_artists === 1 ? "artist" : "artists"}`;
  return `Playing a mix of ${radio.artist} and ${others} from your library.`;
}

export function useArtistRadio(
  account: string,
  player: Pick<Player, "playQueue">,
) {
  const scope = useRequestScope(account);
  const [state, setState] = useState<RadioState>({
    pending: null,
    notice: null,
  });

  async function start(group: Schema<"ArtistIdentityGroup">) {
    const request = scope.capture();
    if (state.pending || !request.active()) return;
    const requestId = createUuid();
    const started = performance.now();
    const controller = new AbortController();
    const cancel = () => controller.abort(request.signal.reason);
    request.signal.addEventListener("abort", cancel, { once: true });
    const timer = setTimeout(
      () => controller.abort(new Error("Radio took too long. Try again.")),
      REQUEST_DEADLINE_MS,
    );
    let outcome = "cancelled";
    setState({ pending: group.id, notice: null });
    log.debug("artist_radio.started", { request_id: requestId });
    try {
      const radio = await radioApi.start(account, group.id, controller.signal);
      if (!request.active()) return;
      if (radio.tracks.length === 0) {
        outcome = "empty";
        setState({
          pending: null,
          notice: {
            tone: "error",
            text: `Nothing playable from ${group.name.value} is in your library right now.`,
          },
        });
        return;
      }
      await player.playQueue(radio.tracks);
      if (!request.active()) return;
      outcome = "playing";
      setState({
        pending: null,
        notice: { tone: "status", text: mixText(radio) },
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
      clearTimeout(timer);
      request.signal.removeEventListener("abort", cancel);
      const record = outcome === "failed" ? log.warn : log.debug;
      record("artist_radio.finished", {
        request_id: requestId,
        reason: outcome,
        duration_ms: Math.round(performance.now() - started),
      });
    }
  }

  return { ...state, start };
}
