import {
  createContext,
  useContext,
  useEffect,
  useState,
  type ReactNode,
} from "react";
import { sourcesApi } from "../../sources/api/sourcesApi";
import { sourcePreferenceApi } from "../../sources/api/sourcePreferenceApi";
import { useNotice } from "../../../shared/ui/Notices";
import { Playback } from "../playback";
import type { MediaPort, Player } from "../model";
import { log } from "../../../shared/log";
import { useRequestScope } from "../../../shared/hooks/useRequestScope";
import { NativePlayback } from "../native/NativePlayback";
import { sourcePreparationApi } from "../../sources/api/sourcePreparationApi";
import { sourceResolutionApi } from "../../sources/api/sourceResolutionApi";
import { useBrowserMediaSession } from "../browser/BrowserMediaSession";
import { BrowserProgress } from "../browser/BrowserProgress";
import { audiobookApi } from "../../audiobooks/api/audiobookApi";
import { BrowserCommunityReporting } from "../browser/BrowserCommunityReporting";
import { QueuePersistence } from "../browser/QueuePersistence";

const PlaybackContext = createContext<Player | null>(null);

export function PlaybackProvider({
  account,
  native = false,
  children,
}: {
  account: string;
  native?: boolean;
  children: ReactNode;
}) {
  return native ? (
    <NativeAccountPlayback key={account} account={account}>
      {children}
    </NativeAccountPlayback>
  ) : (
    <BrowserAccountPlayback key={account} account={account}>
      {children}
    </BrowserAccountPlayback>
  );
}

function BrowserAccountPlayback({
  account,
  children,
}: {
  account: string;
  children: ReactNode;
}) {
  const notice = useNotice();
  const scope = useRequestScope(account);
  const [{ audio, media, player, community }] = useState(() => {
    const audio = new Audio();
    const media = new Map<MediaPort, HTMLAudioElement>([[audio, audio]]);
    let player: Playback;
    const community = new BrowserCommunityReporting(account, scope.capture);
    const progress = new BrowserProgress(
      account,
      scope.capture,
      (message) => {
        player.progressFailed(message);
        notice(message);
      },
      () => player.retryProgress(),
    );
    player = new Playback(
      audio,
      async (recording, options) => {
        if (options.purpose === "resolve")
          return sourceResolutionApi.resolve(
            account,
            recording,
            options.body,
            options.signal,
          );
        return options.purpose === "refresh"
          ? sourcesApi.recoverSources(account, recording, options.signal)
          : sourcesApi.playbackSources(account, recording, options.signal);
      },
      (recording, position, speed, chapter) =>
        progress.admit({ recording, position, speed, chapter }),
      {
        create: () => {
          const next = new Audio();
          next.preload = "auto";
          next.dataset.hitoPreload = "";
          media.set(next, next);
          document.body.append(next);
          return next;
        },
        release: (current) => {
          media.get(current)?.remove();
          media.delete(current);
        },
      },
      async (book, signal) => {
        const current = scope.capture();
        const combined = AbortSignal.any([signal, current.signal]);
        const check = () => {
          combined.throwIfAborted();
          if (!current.active())
            throw new DOMException("Retired", "AbortError");
        };
        check();
        await progress.flush(combined);
        check();
        const playback = await audiobookApi.playback(account, book, combined);
        check();
        return playback;
      },
      (source, claim) => community.admit(source, claim),
      {
        start: (recording, source, body, signal) =>
          sourcePreparationApi.create(account, recording, source, body, signal),
        status: (recording, source, signal) =>
          sourcePreparationApi.status(account, recording, source, signal),
      },
      () => sourcePreferenceApi.fallbackEnabled(account),
    );
    return { audio, media, player, community };
  });
  useBrowserMediaSession(player, account);
  useEffect(() => {
    community.activate();
    log.info("playback.account_opened", { account_id: account });
    media.set(audio, audio);
    audio.dataset.hitoPlayer = "";
    audio.preload = "auto";
    document.body.append(audio);
    const persistence = new QueuePersistence(player, account);
    const restoring = new AbortController();
    let stopPersisting: (() => void) | null = null;
    void persistence
      .restore(restoring.signal)
      .catch(() => log.warn("queue.restore_failed"))
      .finally(() => {
        if (!restoring.signal.aborted) stopPersisting = persistence.start();
      });
    return () => {
      log.info("playback.account_closed", { account_id: account });
      restoring.abort();
      stopPersisting?.();
      player.retire();
      community.close();
      for (const element of media.values()) element.remove();
      media.clear();
    };
  }, [account, audio, media, player, community]);
  return (
    <PlaybackContext.Provider value={player}>
      {children}
    </PlaybackContext.Provider>
  );
}

function NativeAccountPlayback({
  account,
  children,
}: {
  account: string;
  children: ReactNode;
}) {
  const [player] = useState(() => new NativePlayback(account));
  useEffect(() => {
    log.info("playback.account_opened", {
      account_id: account,
      mode: "native",
    });
    void player.reconnect();
    return () => {
      log.info("playback.account_closed", {
        account_id: account,
        mode: "native",
      });
      player.close();
    };
  }, [account, player]);
  return (
    <PlaybackContext.Provider value={player}>
      {children}
    </PlaybackContext.Provider>
  );
}

export function usePlayer() {
  const player = useContext(PlaybackContext);
  if (!player) throw new Error("Playback needs a PlaybackProvider");
  return player;
}
