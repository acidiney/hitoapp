import type { Schema } from "../../../shared/schema";
import { useCallback, useEffect, useRef, useState } from "react";
import { log } from "../../../shared/log";
import { lyricsApi } from "../api/lyricsApi";
import { pluginsApi } from "../../plugins/api/pluginsApi";

type Provider = Schema<"RecordingLyricsProvider">;
type Lines = Schema<"TimedLyrics">;

export type NowPlayingLyricsState =
  | { status: "idle"; providers: Provider[] }
  | { status: "loading"; providers: Provider[] }
  | { status: "no_provider"; providers: Provider[] }
  | { status: "none_for_track"; providers: Provider[] }
  | { status: "failed"; providers: Provider[] }
  | {
      status: "done";
      providers: Provider[];
      provider: Provider;
      lyrics: Lines | null;
      unsupported: boolean;
    };

const DEADLINE_MS = 15_000;

/**
 * Loads the lyrics of the playing track from the account's first lyrics provider as soon as the
 * panel opens. Another provider can be chosen afterwards. Leaving the panel or changing the track
 * cancels whatever is in flight.
 */
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
  const providers = useRef<Provider[]>([]);

  const read = useCallback(
    async (provider: Provider, signal: AbortSignal) => {
      const result = await lyricsApi.read(
        account,
        recording,
        {
          observation_id: provider.observation_id,
          installation_revision: provider.installation_revision,
        },
        signal,
      );
      if (signal.aborted) return;
      setState({
        status: "done",
        providers: providers.current,
        provider,
        lyrics:
          result.status === "ready" ? (result.document.lyrics ?? null) : null,
        unsupported: result.status !== "ready",
      });
    },
    [account, recording],
  );

  const start = useCallback(
    async (observation?: string) => {
      controller.current?.abort();
      const own = new AbortController();
      controller.current = own;
      const timer = setTimeout(() => {
        if (controller.current !== own || own.signal.aborted) return;
        outcome = "deadline";
        own.abort();
        setState({ status: "failed", providers: providers.current });
      }, DEADLINE_MS);
      const started = performance.now();
      setState({ status: "loading", providers: providers.current });
      let outcome = "ok";
      try {
        if (observation === undefined) {
          const listed = await lyricsApi.providers(
            account,
            recording,
            own.signal,
          );
          if (own.signal.aborted) return;
          providers.current = listed.items;
          if (listed.items.length === 0) {
            const settings = await pluginsApi.settings(account, own.signal);
            if (own.signal.aborted) return;
            const connected = settings.installations.some((item) =>
              item.manifest.capabilities.includes("lyrics"),
            );
            outcome = connected ? "none_for_track" : "no_provider";
            setState({
              status: connected ? "none_for_track" : "no_provider",
              providers: [],
            });
            return;
          }
        }
        const chosen =
          providers.current.find(
            (item) => item.observation_id === observation,
          ) ?? providers.current[0];
        await read(chosen, own.signal);
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
