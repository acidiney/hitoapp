import { useCallback, useEffect, useRef, useState } from "react";
import { log } from "../../../shared/log";
import { onInvalidated } from "../../../shared/invalidation";
import { lyricsApi } from "../api/lyricsApi";
import { credentialRequirement } from "../../plugins/model/credentialError";
import { lookupAutomatic, rememberedAutomatic } from "../api/automaticLyrics";
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
import {
  clearCredentialRequirementAfterProtectedSuccess,
  credentialMarkVersion,
  reportCredentialRequirement,
} from "../../plugins/model/credentialRequirements";

export type LyricChoice = {
  key: string;
  label: string;
  identity: LyricIdentity;
};

export type NowPlayingLyricsState =
  | { status: "idle"; providers: LyricChoice[] }
  | { status: "loading"; providers: LyricChoice[] }
  | { status: "no_provider"; providers: LyricChoice[] }
  | { status: "none_for_track"; providers: LyricChoice[] }
  | { status: "failed"; providers: LyricChoice[] }
  | { status: "credentials"; providers: LyricChoice[] }
  | {
      status: "done";
      providers: LyricChoice[];
      provider: { key: string; label: string };
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
      const identity = choice.identity;
      const mark = credentialMarkVersion(account, identity.installation_id);
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
      if (result.status === "ready_v1" || result.status === "ready_v2")
        clearCredentialRequirementAfterProtectedSuccess(
          account,
          identity.installation_id,
          identity.installation_revision,
          mark,
        );
      const outcome: SelectedLyricsOutcome = selectedLyricsOutcome(result);
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
    async (key?: string, force = false) => {
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
            listed.items.length === 0 &&
            automatic &&
            (force || rememberedAutomatic(account, recording) === null)
          ) {
            const found = await lookupAutomatic(account, recording, own.signal);
            if (own.signal.aborted) return;
            outcome = found?.kind ?? "changed";
            if (!found || found.kind === "unavailable") {
              setState({ status: "failed", providers: [] });
              return;
            }
            if (found.kind === "cancelled") {
              setState({ status: "idle", providers: [] });
              return;
            }
            if (found.kind === "lyrics") {
              setState({
                status: "done",
                providers: [],
                provider: { key: "automatic", label: found.label },
                lyrics: found.view,
                unsupported: false,
              });
              return;
            }
            if (found.kind === "unsupported") {
              setState({
                status: "done",
                providers: [],
                provider: { key: "automatic", label: "" },
                lyrics: null,
                unsupported: true,
              });
              return;
            }
            setState({
              status:
                found.kind === "no_services" ? "no_provider" : "none_for_track",
              providers: [],
            });
            return;
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
      } catch (error) {
        const credentials = credentialRequirement(error);
        if (outcome !== "deadline")
          outcome = own.signal.aborted
            ? "cancelled"
            : credentials
              ? "credentials"
              : "failed";
        if (!own.signal.aborted) {
          if (credentials) reportCredentialRequirement(account, credentials);
          setState({
            status: credentials ? "credentials" : "failed",
            providers: providers.current,
          });
        }
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

  useEffect(() => {
    if (!automatic) return;
    const refresh = (id: string | null) => {
      if (id === null || id === recording) void start(undefined, true);
    };
    const stopLyrics = onInvalidated("lyrics", refresh);
    const stopPlugins = onInvalidated("plugins", () => void start());
    return () => {
      stopLyrics();
      stopPlugins();
    };
  }, [start, recording, automatic]);

  return {
    state,
    choose: (key: string) => start(key),
    retry: () => void start(undefined, true),
  };
}
