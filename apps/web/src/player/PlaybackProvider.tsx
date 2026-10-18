import {
  createContext,
  useContext,
  useEffect,
  useState,
  type ReactNode,
} from "react";
import { libraryApi } from "../api";
import { useNotice } from "../components/Notices";
import { Playback, type MediaPort } from "../playback";
import type { Player } from "./model";
import { log } from "../log";
import { useRequestScope } from "../hooks/useRequestScope";
import { NativePlayback } from "./native/NativePlayback";
import { sourceResolutionApi } from "../sourceResolutionApi";
import { useBrowserMediaSession } from "./BrowserMediaSession";
import { BrowserProgress } from "./BrowserProgress";
import { request } from "../http";
import type { components } from "../generated";
import { BrowserCommunityReporting } from "./BrowserCommunityReporting";

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
          return (
            await sourceResolutionApi.resolve(
              account,
              recording,
              options.body,
              options.signal,
            )
          ).sources;
        return options.purpose === "refresh"
          ? libraryApi.recoverSources(account, recording, options.signal)
          : libraryApi.sources(account, recording, options.signal);
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
        activate: (current) => {
          for (const element of media.values())
            element.removeAttribute("data-hito-player");
          const element = media.get(current);
          if (element) {
            element.removeAttribute("data-hito-preload");
            element.dataset.hitoPlayer = "";
          }
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
        const response = await request(
          `/audiobooks/${encodeURIComponent(book)}/playback`,
          {
            action: "audiobooks.playback_read",
            account,
            signal: combined,
          },
        );
        const playback: components["schemas"]["AudiobookPlayback"] =
          await response.json();
        check();
        return playback;
      },
      (source, claim) => community.admit(source, claim),
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
    return () => {
      log.info("playback.account_closed", { account_id: account });
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
