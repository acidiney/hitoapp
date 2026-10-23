import { useEffect, useRef, useSyncExternalStore } from "react";
import { useArtwork } from "../../artwork/components/ArtworkProvider";
import { log } from "../../../shared/log";
import type { Player, Track } from "../model";
import {
  probeSessionArtwork,
  type SessionArtworkCandidate,
} from "./BrowserSessionArtwork";

const actions = [
  "play",
  "pause",
  "seekto",
  "seekbackward",
  "seekforward",
  "previoustrack",
  "nexttrack",
  "stop",
] as const;

export function useBrowserMediaSession(player: Player, account: string) {
  const queue = useSyncExternalStore(player.subscribe, player.queueSnapshot);
  const current = player.snapshot().current;
  const recording = current?.id;
  const release = current?.release_id ?? undefined;
  const releaseArtwork = useArtwork("release", release);
  const recordingArtwork = useArtwork("recording", recording);
  const revision = `${releaseArtwork.revision}:${recordingArtwork.revision}`;
  const connection = useRef<ReturnType<typeof connectSession>>(undefined);
  useEffect(() => {
    const current = connectSession(player, account);
    connection.current = current;
    return () => {
      current?.close();
      if (connection.current === current) connection.current = undefined;
    };
  }, [account, player]);
  useEffect(() => {
    connection.current?.refreshArtwork(recording, release, revision, {
      release: releaseArtwork.missing,
      recording: recordingArtwork.missing,
    });
  }, [
    account,
    player,
    queue.version,
    recording,
    release,
    revision,
    releaseArtwork.missing,
    recordingArtwork.missing,
  ]);
}

function connectSession(player: Player, account: string) {
  if (typeof navigator === "undefined" || !navigator.mediaSession) return;
  const session = navigator.mediaSession;
  let active = true;
  let generation = 0;
  let entry: string | null = null;
  let metadata: Track | null = null;
  let artworkRecording: string | null = null;
  let artworkAttempt: {
    recording: string;
    release?: string;
    revision: string;
  } | null = null;
  let artwork: MediaImage[] = [];
  let stopArtwork = () => {};
  const registered = new Set<MediaSessionAction>();

  function attempt(action: string, command: () => void) {
    try {
      command();
    } catch {
      log.debug("playback.system_unavailable", {
        account_id: account,
        action,
        reason: "unsupported",
      });
    }
  }

  function clearActions() {
    for (const action of registered)
      attempt(action, () => session.setActionHandler(action, null));
    registered.clear();
  }

  function publishPosition() {
    if (!active || typeof session.setPositionState !== "function") return;
    const state = player.snapshot();
    const { position, duration } = player.positionSnapshot();
    const valid =
      state.current &&
      Number.isFinite(duration) &&
      duration > 0 &&
      Number.isFinite(position) &&
      Number.isFinite(state.speed) &&
      state.speed > 0;
    attempt("position", () =>
      session.setPositionState(
        valid
          ? {
              duration,
              position: Math.max(0, Math.min(position, duration)),
              playbackRate: state.speed,
            }
          : undefined,
      ),
    );
  }

  function handle(
    action: (typeof actions)[number],
    details: MediaSessionActionDetails,
    owner: number,
  ) {
    const state = player.snapshot();
    if (
      !active ||
      owner !== generation ||
      !state.current ||
      state.connection !== "ready" ||
      state.controlsAvailable === false
    )
      return;
    const started = performance.now();
    const fields = {
      account_id: account,
      recording_id: state.current.id,
      action,
    };
    try {
      let pending: void | Promise<void> = undefined;
      if (action === "play" || action === "pause") {
        if (state.loading) return;
        pending = player.setPlaying(action === "play");
      } else if (action === "nexttrack") pending = player.next();
      else if (action === "previoustrack") pending = player.previous();
      else if (action === "stop") player.clear();
      else {
        const { duration, position } = player.positionSnapshot();
        if (!Number.isFinite(duration) || duration <= 0) return;
        let target: number;
        if (action === "seekto") {
          if (
            details.seekTime === undefined ||
            !Number.isFinite(details.seekTime)
          )
            return;
          target = details.seekTime;
        } else {
          const offset = details.seekOffset ?? 10;
          if (
            !Number.isFinite(offset) ||
            offset <= 0 ||
            !Number.isFinite(position)
          )
            return;
          target = position + (action === "seekbackward" ? -offset : offset);
        }
        player.seek(Math.max(0, Math.min(target, duration)));
      }
      void Promise.resolve(pending).catch(() => {
        if (active && owner === generation)
          log.warn("playback.system_action_failed", {
            ...fields,
            reason: "command_failed",
            duration_ms: Math.round(performance.now() - started),
          });
      });
      log.debug("playback.system_action", fields);
    } catch {
      log.warn("playback.system_action_failed", {
        ...fields,
        reason: "command_failed",
        duration_ms: Math.round(performance.now() - started),
      });
    }
  }

  function publish() {
    if (!active) return;
    const state = player.snapshot();
    const current = state.current;
    const selected = current ? player.queueSnapshot().currentEntry : null;
    if ((current?.id ?? null) !== artworkRecording) {
      stopArtwork();
      artworkRecording = current?.id ?? null;
      artwork = [];
      artworkAttempt = null;
    }
    if (selected !== entry) {
      entry = selected;
      generation++;
      clearActions();
      if (current && typeof session.setActionHandler === "function") {
        const owner = generation;
        for (const action of actions) {
          if (action === "previoustrack" && !player.supports.previous) continue;
          attempt(action, () => {
            session.setActionHandler(action, (details) =>
              handle(action, details, owner),
            );
            registered.add(action);
          });
        }
      }
    }
    if (
      current?.id !== metadata?.id ||
      current?.title !== metadata?.title ||
      current?.artist !== metadata?.artist ||
      current?.album !== metadata?.album
    ) {
      metadata = current;
      attempt("metadata", () => {
        session.metadata =
          current && typeof MediaMetadata === "function"
            ? new MediaMetadata({
                title: current.title,
                artist: current.artist,
                album: current.album ?? "",
                artwork,
              })
            : null;
      });
      log.debug("playback.system_metadata", {
        account_id: account,
        recording_id: current?.id,
        reason: current ? "selected" : "cleared",
      });
    }
    attempt("state", () => {
      session.playbackState = current
        ? state.playing
          ? "playing"
          : "paused"
        : "none";
    });
    publishPosition();
  }

  function refreshArtwork(
    recording: string | undefined,
    release: string | undefined,
    revision: string,
    missing: { release: () => void; recording: () => void },
  ) {
    if (
      !active ||
      !recording ||
      player.snapshot().current?.id !== recording ||
      typeof Image !== "function"
    )
      return;
    if (
      artworkAttempt?.recording === recording &&
      artworkAttempt.release === release &&
      artworkAttempt.revision === revision
    )
      return;
    const attempt = { recording, release, revision };
    stopArtwork();
    artworkAttempt = attempt;
    artwork = [];
    metadata = null;
    publish();
    const candidates: SessionArtworkCandidate[] = [
      ...(release ? [{ kind: "release" as const, id: release }] : []),
      { kind: "recording", id: recording },
    ];
    const live = () =>
      active &&
      artworkAttempt === attempt &&
      player.snapshot().current?.id === recording;
    stopArtwork = probeSessionArtwork(
      account,
      recording,
      candidates,
      (image) => {
        if (!live()) return;
        artwork = [image];
        metadata = null;
        publish();
      },
      (candidate) => {
        if (live()) missing[candidate.kind]();
      },
    );
  }

  const stopState = player.subscribe(publish);
  const stopPosition = player.subscribePosition(publishPosition);
  publish();
  log.debug("playback.system_opened", { account_id: account });
  return {
    refreshArtwork,
    close() {
      active = false;
      generation++;
      stopState();
      stopPosition();
      stopArtwork();
      clearActions();
      attempt("metadata", () => {
        session.metadata = null;
      });
      attempt("state", () => {
        session.playbackState = "none";
      });
      if (typeof session.setPositionState === "function")
        attempt("position", () => session.setPositionState());
      log.debug("playback.system_closed", { account_id: account });
    },
  };
}
