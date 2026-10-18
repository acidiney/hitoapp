import { useEffect, useMemo, useRef, useState } from "react";
import { audiobookApi } from "../audiobookApi";
import { log } from "../log";
import { usePlayer } from "../player/PlaybackProvider";
import type { AudiobookDetail, PlaylistEntry } from "../types";
import { createUuid } from "../uuid";

type Bookmark = AudiobookDetail["bookmarks"][number];

export function useAudiobookPlayback(
  account: string,
  detail: AudiobookDetail,
  rendered: AudiobookDetail,
) {
  const player = usePlayer();
  const owner = useMemo(
    () => ({
      controller: null as AbortController | null,
      active: true,
      pending: false,
      error: "",
    }),
    [account, detail, player, rendered],
  );
  const latest = useRef(owner);
  latest.current = owner;
  const [, update] = useState(0);
  const publish = () => update((revision) => revision + 1);
  useEffect(() => {
    owner.active = true;
    return () => {
      owner.active = false;
      owner.controller?.abort();
    };
  }, [owner]);

  async function play(
    chapter: PlaylistEntry,
    bookmark?: Bookmark,
  ): Promise<void> {
    if (owner.pending || !chapter.recording_id || !chapter.available) return;
    const select = player.prepareSelection();
    const controller = new AbortController();
    owner.controller = controller;
    owner.pending = true;
    owner.error = "";
    publish();
    const current = () => owner.active && latest.current === owner;
    const active = () => current() && !controller.signal.aborted;
    const started = performance.now();
    const fields = {
      request_id: createUuid(),
      action: bookmark ? "bookmark" : "chapter",
      mode: player.mode,
    };
    let outcome = "retired";
    let count = 0;
    const deadline = setTimeout(() => controller.abort(), 10_000);
    let cancel!: () => void;
    const cancelled = new Promise<never>((_, reject) => {
      cancel = () => reject(new DOMException("Cancelled", "AbortError"));
      controller.signal.addEventListener("abort", cancel, { once: true });
    });
    log.info("audiobooks.playback_requested", fields);
    try {
      const checked = await Promise.race([
        audiobookApi.read(account, detail.playlist.id, controller.signal),
        cancelled,
      ]);
      clearTimeout(deadline);
      if (!active()) return;
      const chapters = checked.chapters.filter(
        (entry) => entry.recording_id != null,
      );
      const index = chapters.findIndex(
        (entry) => entry.entry_id === chapter.entry_id,
      );
      const target = chapters[index];
      const marker = bookmark
        ? checked.bookmarks.find((marker) => marker.id === bookmark.id)
        : undefined;
      const markerMatches =
        !bookmark ||
        (marker?.audiobook_id === detail.playlist.id &&
          marker.entry_id === chapter.entry_id &&
          marker.recording_id === chapter.recording_id &&
          marker.position_ms === bookmark.position_ms &&
          Number.isSafeInteger(marker.position_ms) &&
          marker.position_ms >= 0);
      if (
        !target ||
        target.recording_id !== chapter.recording_id ||
        !target.available ||
        !markerMatches
      ) {
        outcome = "unavailable";
        owner.error = bookmark
          ? "This bookmark is no longer available. Refresh the audiobook."
          : "This chapter is no longer available. Refresh the audiobook.";
        return;
      }
      const queue = chapters.map((entry) => ({
        ...entry,
        id: entry.recording_id!,
      }));
      count = queue.length;
      outcome = (await select({
        queue,
        index,
        start: {
          kind: "position",
          position: (marker?.position_ms ?? 0) / 1000,
        },
      }))
        ? "handed_off"
        : "superseded";
    } catch {
      outcome = controller.signal.aborted ? "cancelled" : "read_failed";
      if (current())
        owner.error = "Hito could not check this audiobook. Try playing again.";
    } finally {
      clearTimeout(deadline);
      controller.signal.removeEventListener("abort", cancel);
      owner.controller = null;
      owner.pending = false;
      if (current()) publish();
      log.debug("audiobooks.playback_finished", {
        ...fields,
        count,
        reason: outcome,
        duration_ms: Math.round(performance.now() - started),
      });
    }
  }

  function playChapter(chapter: PlaylistEntry) {
    return play(chapter);
  }

  async function playBookmark(bookmark: Bookmark) {
    const chapter = detail.chapters.find(
      (chapter) =>
        chapter.entry_id === bookmark.entry_id &&
        chapter.recording_id === bookmark.recording_id,
    );
    if (chapter && bookmark.audiobook_id === detail.playlist.id)
      await play(chapter, bookmark);
  }

  return {
    pending: owner.pending,
    error: owner.error,
    playChapter,
    playBookmark,
  };
}
