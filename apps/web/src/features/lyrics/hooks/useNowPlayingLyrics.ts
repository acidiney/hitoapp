import { useCallback, useEffect, useRef, useState } from "react";
import { log } from "../../../shared/log";
import { lyricsApi } from "../api/lyricsApi";
import {
  listCompatibleChoices,
  readCompatibleChoice,
  type CompatibleChoice,
} from "../api/compatibleLyrics";
import {
  readMatchesIdentity,
  selectedLyricsOutcome,
  type LyricView,
  type SelectedLyricsOutcome,
} from "../model/lyricContent";
import {
  identityKey,
  identityNames,
  type LyricIdentity,
} from "../model/lyricIdentity";

export type LyricChoice = {
  key: string;
  label: string;
  identity: LyricIdentity | null;
  compatible: CompatibleChoice | null;
};

export type NowPlayingLyricsState =
  | { status: "idle"; providers: LyricChoice[] }
  | { status: "loading"; providers: LyricChoice[] }
  | { status: "no_provider"; providers: LyricChoice[] }
  | { status: "none_for_track"; providers: LyricChoice[] }
  | { status: "failed"; providers: LyricChoice[] }
  | {
      status: "done";
      providers: LyricChoice[];
      provider: LyricChoice;
      lyrics: LyricView | null;
      unsupported: boolean;
    };

const DEADLINE_MS = 15_000;

function identityChoices(identities: LyricIdentity[]): LyricChoice[] {
  const names = identityNames(identities);
  return identities.map((identity) => ({
    key: identityKey(identity),
    label: names[identityKey(identity)],
    identity,
    compatible: null,
  }));
}

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
  const providers = useRef<LyricChoice[]>([]);

  const read = useCallback(
    async (
      choice: LyricChoice,
      signal: AbortSignal,
    ): Promise<SelectedLyricsOutcome["kind"] | "changed"> => {
      let outcome: SelectedLyricsOutcome;
      if (choice.identity) {
        const identity = choice.identity;
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
        outcome = selectedLyricsOutcome(result);
      } else {
        const result = await readCompatibleChoice(
          account,
          recording,
          choice.compatible!,
          signal,
        );
        if (signal.aborted) return "cancelled";
        if (result.kind === "changed") {
          setState({ status: "failed", providers: providers.current });
          return "changed";
        }
        outcome = result.outcome;
      }
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
        provider: choice,
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
          providers.current = identityChoices(
            listed.items.filter((item) => item.selectable),
          );
          if (
            providers.current.length === 0 &&
            automatic &&
            listed.items.length === 0
          ) {
            const found = await listCompatibleChoices(
              account,
              recording,
              own.signal,
            );
            if (own.signal.aborted) return;
            if (found === "changed") throw new Error("changed");
            providers.current = found.map((item) => ({
              key: item.key,
              label: item.label,
              identity: null,
              compatible: item,
            }));
          }
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
          providers.current.find((item) => item.key === key) ??
          providers.current[0];
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
    [account, recording, automatic, read],
  );

  useEffect(() => {
    providers.current = [];
    if (automatic) void start();
    else setState({ status: "idle", providers: [] });
    return () => controller.current?.abort();
  }, [start, automatic]);

  return { state, choose: start, retry: () => void start() };
}
