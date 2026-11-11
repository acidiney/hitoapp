import { useEffect, useRef, useState, useSyncExternalStore } from "react";
import { router } from "@inertiajs/react";
import { log } from "../shared/log";
import { visitFromPlayer } from "../shared/ui/RouteLink";
import { usePlayer } from "../features/playback/components/PlaybackProvider";
import { useTrackActions } from "../features/playback/components/TrackActionsProvider";
import { requestNowPlaying } from "../features/playback/nowPlayingRequests";
import { useMute } from "../features/playback/hooks/useMute";
import { useToggleLike } from "../features/playback/hooks/useToggleLike";
import ShortcutsDialog from "./ShortcutsDialog";
import { useSingleKeyShortcuts } from "./shortcutPreference";
import {
  nextChord,
  resolveShortcut,
  type ShortcutCommand,
  type ShortcutState,
} from "./shortcuts";

const CHORD_MS = 1500;

const noMutation = async () => ({ ok: false, error: "unavailable" }) as const;

export default function ShellShortcuts({
  open,
  onOpenChange,
}: {
  open: boolean;
  onOpenChange: (open: boolean) => void;
}) {
  const player = usePlayer();
  const actions = useTrackActions();
  const toggleLike = useToggleLike(actions?.mutate ?? noMutation);
  const current = useSyncExternalStore(
    player.subscribe,
    () => player.snapshot().current,
  );
  const [singleKeys, setSingleKeys] = useSingleKeyShortcuts();
  const toggleMute = useMute();
  const handler = useRef<(command: ShortcutCommand) => void>(() => {});
  handler.current = (command) => {
    log.debug("shortcut.used", { action: command });
    if (command === "help") onOpenChange(true);
    else if (command === "togglePlay") void player.toggle();
    else if (command === "next") void player.next();
    else if (command === "previous") void player.previous();
    else if (command === "queue") visitFromPlayer("/queue");
    else if (command === "nowPlaying") requestNowPlaying();
    else if (command === "home") router.visit("/home");
    else if (command === "library") router.visit("/library");
    else if (command === "plugins") router.visit("/settings?section=plugins");
    else if (command === "mute") toggleMute();
    else if (command === "like" && current && actions) void toggleLike(current);
  };
  useEffect(() => {
    let chord = false;
    let timer: number | undefined;
    const clear = () => {
      chord = false;
      window.clearTimeout(timer);
    };
    function listen(event: KeyboardEvent) {
      const state: ShortcutState = { chord, singleKeys };
      const command = resolveShortcut(event, state);
      const starting = nextChord(event, state);
      clear();
      if (starting) {
        chord = true;
        timer = window.setTimeout(clear, CHORD_MS);
      }
      if (!command) return;
      event.preventDefault();
      handler.current(command);
    }
    window.addEventListener("keydown", listen);
    return () => {
      window.removeEventListener("keydown", listen);
      clear();
    };
  }, [singleKeys]);
  return open ? (
    <ShortcutsDialog
      singleKeys={singleKeys}
      onSingleKeys={setSingleKeys}
      onClose={() => onOpenChange(false)}
    />
  ) : null;
}
