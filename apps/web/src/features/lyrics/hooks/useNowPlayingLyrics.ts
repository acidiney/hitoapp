import { useCallback, useEffect, useRef, useState } from "react";
import { log } from "../../../shared/log";
import { lyricsApi } from "../api/lyricsApi";
import {
  readMatchesIdentity,
  selectedLyricsOutcome,
  type LyricView,
} from "../model/lyricContent";
import {
  firstSelectable,
  identityKey,
  type LyricIdentity,
} from "../model/lyricIdentity";

export type NowPlayingLyricsState =
  | { status: "idle"; providers: LyricIdentity[] }
  | { status: "loading"; providers: LyricIdentity[] }
  | { status: "no_provider"; providers: LyricIdentity[] }
  | { status: "none_for_track"; providers: LyricIdentity[] }
  | { status: "failed"; providers: LyricIdentity[] }
  | {
      status: "done";
      providers: LyricIdentity[];
      provider: LyricIdentity;
      lyrics: LyricView | null;
      unsupported: boolean;
    };

const DEADLINE_MS = 15_000;

export function useNowPlayingLyrics(
  account: string,
  recording: string,
  automatic = true,
) {
  const [state, setState] = useState<NowPlayingLyricsState>({
    status: "loading",
    providers: [],
  });
  const controller = useRef<AbortController | null>(null);
  const providers = useRef<LyricIdentity[]>([]);

  const read = useCallback(
    async (identity: LyricIdentity, signal: AbortSignal) => {
      const result = await lyricsApi.readSelected(
        account,
        recording,
        {
          selector: identity.selector,
          installation_revision: identity.installation_revision,
        },
        signal,
      );
      if (signal.aborted) return "cancelled";
      if (
        (result.status === "ready_v1" || result.status === "ready_v2") &&
        !readMatchesIdentity(result, recording, identity)
      ) {
        setState({ status: "failed", providers: providers.current });
        return "changed";
      }
      const outcome = selectedLyricsOutcome(result);
      if (outcome.kind === "unavailable" || outcome.kind === "cancelled") {
        setState({
          status: outcome.kind === "cancelled" ? "idle" : "failed",
          providers: providers.current,
        });
        return outcome.kind;
      }
      setState({
        status: "done",
        providers: providers.current,
        provider: identity,
        lyrics: outcome.kind === "lyrics" ? outcome.view : null,
        unsupported: outcome.kind === "unsupported",
      });
      return outcome.kind;
    },
    [account, recording],
  );

  const start = useCallback(
    async (key?: string) => {
      controller.current?.abort();
      const own = new AbortController();
      controller.current = own;
      let outcome = "ok";
      const timer = setTimeout(() => {
        if (controller.current !== own || own.signal.aborted) return;
        outcome = "deadline";
        own.abort();
        setState({ status: "failed", providers: providers.current });
      }, DEADLINE_MS);
      const started = performance.now();
      setState({ status: "loading", providers: providers.current });
      try {
        if (key === undefined) {
          const listed = await lyricsApi.identities(
            account,
            recording,
            own.signal,
          );
          if (own.signal.aborted) return;
          if (listed.recording_id !== recording) throw new Error("changed");
          providers.current = listed.items.filter((item) => item.selectable);
          if (providers.current.length === 0) {
            const services = await lyricsApi.services(
              account,
              recording,
              own.signal,
            );
            if (own.signal.aborted) return;
            const connected = services.items.length > 0;
            outcome = connected ? "none_for_track" : "no_provider";
            setState({
              status: connected ? "none_for_track" : "no_provider",
              providers: [],
            });
            return;
          }
        }
        const chosen =
          providers.current.find((item) => identityKey(item) === key) ??
          firstSelectable(providers.current)!;
        outcome = await read(chosen, own.signal);
      } catch {
        if (outcome !== "deadline")
          outcome = own.signal.aborted ? "cancelled" : "failed";
        if (!own.signal.aborted)
          setState({ status: "failed", providers: providers.current });
      } finally {
        clearTimeout(timer);
        log.debug("lyrics.now_playing", {
          reason: outcome,
          duration_ms: Math.round(performance.now() - started),
        });
      }
    },
    [account, recording, read],
  );

  useEffect(() => {
    providers.current = [];
    if (automatic) void start();
    else setState({ status: "idle", providers: [] });
    return () => controller.current?.abort();
  }, [start, automatic]);

  return { state, choose: start, retry: () => void start() };
}
