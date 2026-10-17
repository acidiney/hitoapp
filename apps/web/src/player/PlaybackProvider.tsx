import {
  createContext,
  useContext,
  useEffect,
  useState,
  type ReactNode,
} from "react";
import { change, libraryApi } from "../api";
import { errorMessage } from "../format";
import { useNotice } from "../components/Notices";
import { Playback } from "../playback";
import type { Player } from "./model";
import { log } from "../log";
import { useRequestScope } from "../hooks/useRequestScope";
import { NativePlayback } from "./native/NativePlayback";

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
  const [{ audio, player }] = useState(() => {
    const audio = new Audio();
    const player = new Playback(
      audio,
      (recording, options) =>
        options.refresh
          ? libraryApi.recoverSources(account, recording, options.signal)
          : libraryApi.sources(account, recording, options.signal),
      (track, position, speed) => {
        const request = scope.capture();
        void change(
          account,
          {
            kind: "progress_set",
            recording_id: track.id,
            position_ms: Math.floor(position * 1000),
            speed,
          },
          request.signal,
        ).catch((error) => {
          if (request.active()) notice(errorMessage(error));
        });
      },
    );
    return { audio, player };
  });
  useEffect(() => {
    log.info("playback.account_opened", { account_id: account });
    audio.dataset.hitoPlayer = "";
    audio.preload = "auto";
    document.body.append(audio);
    return () => {
      log.info("playback.account_closed", { account_id: account });
      player.clear();
      audio.remove();
    };
  }, [account, audio, player]);
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
